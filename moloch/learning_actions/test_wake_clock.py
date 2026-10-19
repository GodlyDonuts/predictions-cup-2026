import random,unittest
from moloch.learning_v2.clock import EventClock
from .wake_clock import WakeClock,OBSERVABLE


class WakeClockTests(unittest.TestCase):
    def test_index_matches_original_heap_scan_through_ties_and_nested_events(self):
        old=EventClock(0);new=WakeClock(0);rng=random.Random(93);a=[];b=[]
        kinds=tuple(sorted(OBSERVABLE))+('api_snapshot_world','api_snapshot_information','tape')
        for i in range(1000):
            at=rng.randrange(10000);priority=rng.randrange(5);kind=rng.choice(kinds)
            old.schedule(at,priority,kind,i);new.schedule(at,priority,kind,i)
        for at in range(0,10001,29):
            def consume(clock,result):
                def f(t,k,p):
                    result.append((t,k,p))
                    if p%59==0 and k!='api_receipt':clock.schedule(t,8,'api_receipt',p)
                return f
            old.advance(at,consume(old,a));new.advance(at,consume(new,b))
            expected=min((t for t,_,_,k,_ in old._events if t>old.now and k in OBSERVABLE),default=None)
            self.assertEqual(new.next_observable(),expected)
            self.assertEqual(a,b);self.assertEqual(old.now,new.now)

    def test_current_timestamp_and_hidden_only_events_do_not_create_future_wakeup(self):
        clock=WakeClock(0);clock.schedule(0,1,'api_receipt',None)
        clock.schedule(1,1,'api_snapshot_world',None);clock.schedule(100,1,'api_receipt',None)
        self.assertEqual(clock.next_observable(),100)
        clock.advance(100,lambda *x:None);self.assertIsNone(clock.next_observable());self.assertFalse(clock._observable)

    def test_failed_scheduling_does_not_leave_phantom_index_entry(self):
        clock=WakeClock(10)
        with self.assertRaises(ValueError):clock.schedule(9,1,'api_receipt',None)
        self.assertFalse(clock._observable);self.assertFalse(clock._events)

    def test_long_wait_processes_more_than_one_million_finite_source_events(self):
        clock=WakeClock(0);last=1_000_001;seen=[0]
        def consume(at,kind,payload):
            seen[0]+=1
            if at<last:clock.schedule(at+1,10,'finite_source',None)
        clock.schedule(0,10,'finite_source',None)
        self.assertEqual(clock.advance(last,consume),last+1)
        self.assertEqual(seen[0],last+1);self.assertEqual(clock.now,last)
