"""Read aggregate metrics for an owned Windows process tree, using Win32 APIs.

Structures: Microsoft Learn PROCESS_MEMORY_COUNTERS_EX and PROCESSENTRY32W.
Working-set sums may count shared pages more than once; private commit is separate.
"""
import ctypes as c
from ctypes import wintypes as w


class ProcessEntry(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
                ("heap", c.c_size_t), ("module", w.DWORD), ("threads", w.DWORD),
                ("parent", w.DWORD), ("priority", w.LONG), ("flags", w.DWORD),
                ("exe", w.WCHAR * 260)]


class Memory(c.Structure):
    _fields_ = [("cb", w.DWORD), ("faults", w.DWORD)] + [
        (name, c.c_size_t) for name in ("peak_rss", "rss", "peak_paged", "paged",
                                      "peak_nonpaged", "nonpaged", "commit", "peak_commit", "private")]


kernel = c.WinDLL("kernel32", use_last_error=True)
psapi = c.WinDLL("psapi", use_last_error=True)
kernel.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
kernel.CreateToolhelp32Snapshot.restype = w.HANDLE
kernel.Process32FirstW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]
kernel.Process32NextW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]
kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel.OpenProcess.restype = w.HANDLE
kernel.CloseHandle.argtypes = [w.HANDLE]
kernel.GetProcessTimes.argtypes = [w.HANDLE] + [c.POINTER(w.FILETIME)] * 4
psapi.GetProcessMemoryInfo.argtypes = [w.HANDLE, c.POINTER(Memory), w.DWORD]


class JobLimits(c.Structure):
    _fields_ = [("process_time", c.c_longlong), ("job_time", c.c_longlong),
                ("flags", w.DWORD), ("min_working_set", c.c_size_t),
                ("max_working_set", c.c_size_t), ("active_process_limit", w.DWORD),
                ("affinity", c.c_size_t), ("priority", w.DWORD), ("scheduling", w.DWORD)]


class JobExtendedLimits(c.Structure):
    _fields_ = [("basic", JobLimits), ("io_counters", c.c_ulonglong * 6),
                ("process_memory", c.c_size_t), ("job_memory", c.c_size_t),
                ("peak_process_memory", c.c_size_t), ("peak_job_memory", c.c_size_t)]


class OwnedJob:
    """Close only processes assigned to this test's Windows job, without PID reuse.

    https://learn.microsoft.com/windows/win32/procthread/job-objects
    The handle stays private and non-inheritable. Child processes inherit the job.
    """
    def __init__(self):
        kernel.CreateJobObjectW.argtypes = [c.c_void_p, w.LPCWSTR]
        kernel.CreateJobObjectW.restype = w.HANDLE
        kernel.SetInformationJobObject.argtypes = [w.HANDLE, c.c_int, c.c_void_p, w.DWORD]
        kernel.AssignProcessToJobObject.argtypes = [w.HANDLE, w.HANDLE]
        self.handle = kernel.CreateJobObjectW(None, None)
        if not self.handle:
            raise c.WinError(c.get_last_error())
        limits = JobExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not kernel.SetInformationJobObject(self.handle, 9, c.byref(limits), c.sizeof(limits)):
            error = c.WinError(c.get_last_error())
            self.close()
            raise error

    def assign(self, process):
        if not kernel.AssignProcessToJobObject(self.handle, int(process._handle)):
            raise c.WinError(c.get_last_error())

    def close(self):
        if self.handle:
            kernel.CloseHandle(self.handle)
            self.handle = None


def process_tree(root):
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    if snapshot == w.HANDLE(-1).value:
        raise c.WinError(c.get_last_error())
    parents = {}
    try:
        entry = ProcessEntry()
        entry.size = c.sizeof(entry)
        present = kernel.Process32FirstW(snapshot, c.byref(entry))
        while present:
            parents[entry.pid] = entry.parent
            present = kernel.Process32NextW(snapshot, c.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    selected = {root}
    while True:
        expanded = selected | {pid for pid, parent in parents.items() if parent in selected}
        if expanded == selected:
            return selected
        selected = expanded


def sample(root):
    rss = private = 0
    cpu = 0.0
    count = 0
    for pid in process_tree(root):
        handle = kernel.OpenProcess(0x410, False, pid)
        if not handle:
            if pid == root:
                raise c.WinError(c.get_last_error())
            continue
        try:
            mem = Memory()
            mem.cb = c.sizeof(mem)
            if not psapi.GetProcessMemoryInfo(handle, c.byref(mem), mem.cb):
                raise c.WinError(c.get_last_error())
            times = [w.FILETIME() for _ in range(4)]
            if not kernel.GetProcessTimes(handle, *(c.byref(t) for t in times)):
                raise c.WinError(c.get_last_error())
            cpu += sum((t.dwHighDateTime << 32) | t.dwLowDateTime for t in times[2:]) / 10_000_000
            rss += mem.rss
            private += mem.private
            count += 1
        finally:
            kernel.CloseHandle(handle)
    return {"rss_mib": rss / 1024**2, "private_mib": private / 1024**2,
            "cpu_seconds": cpu, "processes": count}
