"""A retained Windows handle for one exact local simulated-training process.

No process-name search, tree kill, SIG worker control or PID-only termination.
"""
import ctypes,json,os,subprocess
from pathlib import Path
from ctypes import wintypes as w


class TrainingProcess:
    def __init__(self,pid,expected_run,expected_config,*,expected_creation=None,
                 expected_module='moloch.learning_actions.experiment'):
        if expected_module not in ('moloch.learning_actions.experiment','moloch.learning_actions.asynchronous_islands'):
            raise ValueError('EXPLICIT_SIMULATED_TRAINING_MODULE_REQUIRED')
        if os.name!='nt':raise ValueError('WINDOWS_TRAINING_HANDLE_REQUIRED')
        if type(pid)is not int or pid<=0:raise ValueError('POSITIVE_EXACT_TRAINING_PID')
        self.kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        self.kernel.OpenProcess.argtypes=[w.DWORD,w.BOOL,w.DWORD];self.kernel.OpenProcess.restype=w.HANDLE
        self.kernel.CloseHandle.argtypes=[w.HANDLE]
        self.kernel.GetExitCodeProcess.argtypes=[w.HANDLE,ctypes.POINTER(w.DWORD)]
        self.kernel.GetProcessTimes.argtypes=[w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)]
        self.kernel.TerminateProcess.argtypes=[w.HANDLE,w.UINT]
        self.handle=self.kernel.OpenProcess(0x1000|0x100000|0x0001,False,pid)
        if not self.handle:raise OSError(ctypes.get_last_error(),'EXACT_TRAINING_PROCESS_UNAVAILABLE')
        try:
            times=[w.FILETIME()for _ in range(4)]
            if not self.kernel.GetProcessTimes(self.handle,*map(ctypes.byref,times)):raise OSError('PROCESS_CREATION_UNAVAILABLE')
            self.creation=times[0].dwLowDateTime+(times[0].dwHighDateTime<<32)
            if expected_creation is not None and self.creation!=expected_creation:raise ValueError('TRAINING_PROCESS_CREATION_CHANGED')
            self.kernel.QueryFullProcessImageNameW.argtypes=[w.HANDLE,w.DWORD,w.LPWSTR,ctypes.POINTER(w.DWORD)]
            image=ctypes.create_unicode_buffer(32768);length=w.DWORD(len(image))
            if not self.kernel.QueryFullProcessImageNameW(self.handle,0,image,ctypes.byref(length)):
                raise OSError('TRAINING_EXECUTABLE_UNAVAILABLE')
            if Path(image.value).name.lower()not in('python.exe','python3.exe'):raise ValueError('TARGET_IS_NOT_PYTHON_TRAINING')
            self.executable=image.value
            cmd=f"Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}' | Select-Object CommandLine | ConvertTo-Json -Compress"
            result=subprocess.run(['pwsh','-NoProfile','-NonInteractive','-Command',cmd],capture_output=True,text=True,check=True,timeout=30)
            data=json.loads(result.stdout);command=data['CommandLine']
            shell=ctypes.WinDLL('shell32',use_last_error=True)
            shell.CommandLineToArgvW.argtypes=[w.LPCWSTR,ctypes.POINTER(ctypes.c_int)]
            shell.CommandLineToArgvW.restype=ctypes.POINTER(w.LPWSTR)
            n=ctypes.c_int();args=shell.CommandLineToArgvW(command,ctypes.byref(n))
            if not args:raise ValueError('TRAINING_ARGUMENTS_UNAVAILABLE')
            try:argv=[args[i]for i in range(n.value)]
            finally:
                self.kernel.LocalFree.argtypes=[w.HLOCAL];self.kernel.LocalFree(args)
            def option(name):
                if argv.count(name)!=1 or argv.index(name)+1>=len(argv):raise ValueError('EXACT_TRAINING_ARGUMENT_REQUIRED')
                return argv[argv.index(name)+1]
            if option('-m')!=expected_module:raise ValueError('TARGET_IS_NOT_RAW_API_TRAINING')
            if Path(option('--output')).resolve()!=Path(expected_run).resolve()or Path(option('--config')).resolve()!=Path(expected_config).resolve():
                raise ValueError('TARGET_IS_NOT_EXPECTED_TRAINING_RUN')
            if not self.alive():raise ValueError('TRAINING_PROCESS_ALREADY_EXITED')
            self.pid=pid
        except BaseException:
            self.close();raise

    def alive(self):
        code=w.DWORD()
        if not self.kernel.GetExitCodeProcess(self.handle,ctypes.byref(code)):raise OSError('EXACT_PROCESS_STATUS_UNAVAILABLE')
        return code.value==259

    def terminate_for_handoff(self):
        if not self.alive():raise ValueError('DO_NOT_RESTART_AN_UNEXPECTEDLY_STOPPED_TRAINER')
        if not self.kernel.TerminateProcess(self.handle,42):raise OSError('OWN_TRAINING_PROCESS_DID_NOT_STOP')

    def close(self):
        if getattr(self,'handle',None):self.kernel.CloseHandle(self.handle);self.handle=None
