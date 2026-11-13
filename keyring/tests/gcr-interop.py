#!/usr/bin/env python3
"""Checks keyring/src/exchange.rs against gcr's real secret exchange
(libgcr-base-3): gcr begins, the Rust side answers with a password, gcr
receives it. Run: python3 -I keyring/tests/gcr-interop.py  (from the repo root)"""
import ctypes, subprocess, sys

gcr = ctypes.CDLL("libgcr-base-3.so.1")
glib = ctypes.CDLL("libglib-2.0.so.0")
gcr.gcr_secret_exchange_new.restype = ctypes.c_void_p
gcr.gcr_secret_exchange_new.argtypes = [ctypes.c_char_p]
gcr.gcr_secret_exchange_begin.restype = ctypes.c_void_p
gcr.gcr_secret_exchange_begin.argtypes = [ctypes.c_void_p]
gcr.gcr_secret_exchange_receive.restype = ctypes.c_bool
gcr.gcr_secret_exchange_receive.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
gcr.gcr_secret_exchange_get_secret.restype = ctypes.c_void_p
gcr.gcr_secret_exchange_get_secret.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)]

for password in ["hunter2", "pässwörd with spaces ✓", "x" * 100, ""]:
    # A fresh conversation each time: gcr keeps the first peer's key.
    ex = gcr.gcr_secret_exchange_new(None)
    ptr = gcr.gcr_secret_exchange_begin(ex)
    begin = ctypes.string_at(ptr).decode()
    glib.g_free(ctypes.c_void_p(ptr))
    out = subprocess.run(
        ["cargo", "run", "-q", "-p", "patina-keyring", "--example", "interop", "--", password],
        input=begin, capture_output=True, text=True, check=True).stdout
    ok = gcr.gcr_secret_exchange_receive(ex, out.encode())
    n = ctypes.c_size_t()
    got = ctypes.string_at(gcr.gcr_secret_exchange_get_secret(ex, ctypes.byref(n)), n.value).decode()
    print(("ok  " if ok and got == password else "FAIL"), repr(password), "->", repr(got))
    if not (ok and got == password):
        sys.exit(1)
