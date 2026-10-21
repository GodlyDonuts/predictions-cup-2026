"""One parent owns neural compute; independent policies retain private RNG.

Binding a session does not hold the compute lock. Only explicit neural sections
do, so a policy waiting for its own source processes cannot monopolize compute.
The default, unbound path preserves ordinary single-policy behavior.
"""
from contextlib import contextmanager
from functools import wraps
import os
import threading
import torch

_local = threading.local()
_compute_lock = threading.RLock()
_bound_sessions = 0


class NeuralCoordinator:
    def __init__(self, device='cpu'):
        self.device = torch.device(device)
        if self.device.type not in ('cpu', 'cuda'):
            raise ValueError('SUPPORTED_NEURAL_SESSION_DEVICE')
        self.pid = os.getpid()
        self.lock = _compute_lock
        self.names = set()
        self.cancelled_reason = None

    def cancel(self, reason):
        if type(reason) is not str or not reason:
            raise ValueError('EXPLICIT_NEURAL_CANCELLATION_REASON')
        with self.lock:
            if self.cancelled_reason is None: self.cancelled_reason = reason

    def session(self, name, seed):
        if type(name) is not str or not name or type(seed) is not int:
            raise ValueError('NAMED_INTEGER_SEEDED_NEURAL_SESSION')
        with self.lock:
            if self.cancelled_reason is not None:
                raise RuntimeError('COORDINATED_NEURAL_RUN_CANCELLED')
            if name in self.names:
                raise ValueError('UNIQUE_NEURAL_SESSION_REQUIRED')
            session = NeuralSession(self, name, seed)
            self.names.add(name)
            return session


class NeuralSession:
    def __init__(self, coordinator, name, seed):
        self.coordinator = coordinator
        self.name = name
        self.owner_thread = None
        self.cpu_state = torch.Generator(device='cpu').manual_seed(seed).get_state()
        self.cuda_state = (torch.Generator(device=coordinator.device).manual_seed(seed).get_state()
                           if coordinator.device.type == 'cuda' else None)

    @contextmanager
    def bind(self):
        global _bound_sessions
        c = self.coordinator
        with c.lock:
            if c.cancelled_reason is not None:
                raise RuntimeError('COORDINATED_NEURAL_RUN_CANCELLED')
            if c.pid != os.getpid() or self.owner_thread is not None or getattr(_local, 'session', None) is not None:
                raise ValueError('ONE_THREAD_PER_PARENT_NEURAL_SESSION')
            self.owner_thread = threading.get_ident()
            _local.session = self
            _bound_sessions += 1
        try:
            yield self
        finally:
            with c.lock:
                _local.session = None
                self.owner_thread = None
                _bound_sessions -= 1

    @contextmanager
    def compute(self):
        c = self.coordinator
        if c.pid != os.getpid() or self.owner_thread != threading.get_ident():
            raise ValueError('BOUND_PARENT_NEURAL_SESSION_REQUIRED')
        if getattr(_local, 'computing', None) is self:
            if c.cancelled_reason is not None:
                raise RuntimeError('COORDINATED_NEURAL_RUN_CANCELLED')
            yield
            return
        with c.lock:
            if c.cancelled_reason is not None:
                raise RuntimeError('COORDINATED_NEURAL_RUN_CANCELLED')
            cpu = torch.get_rng_state()
            cuda = torch.cuda.get_rng_state(c.device) if self.cuda_state is not None else None
            try:
                torch.set_rng_state(self.cpu_state)
                if cuda is not None: torch.cuda.set_rng_state(self.cuda_state, c.device)
                _local.computing = self
                yield
            finally:
                self.cpu_state = torch.get_rng_state()
                if cuda is not None: self.cuda_state = torch.cuda.get_rng_state(c.device)
                torch.set_rng_state(cpu)
                if cuda is not None: torch.cuda.set_rng_state(cuda, c.device)
                _local.computing = None


@contextmanager
def neural_section():
    session = getattr(_local, 'session', None)
    if session is None:
        with _compute_lock:
            if _bound_sessions:
                raise ValueError('UNBOUND_NEURAL_WORK_DURING_COORDINATED_RUN')
            yield
    else:
        with session.compute(): yield


def neural_only(function):
    @wraps(function)
    def call(*args, **kwargs):
        with neural_section(): return function(*args, **kwargs)
    return call


def seed_neural(seed):
    session = getattr(_local, 'session', None)
    if session is None:
        with neural_section(): return torch.manual_seed(seed)
    with session.compute():
        generator = torch.Generator(device='cpu').manual_seed(seed)
        torch.set_rng_state(generator.get_state())
        if session.cuda_state is not None:
            device = session.coordinator.device
            torch.cuda.set_rng_state(torch.Generator(device=device).manual_seed(seed).get_state(), device)
        return generator
