import socket, sys, threading, time
listen, target, rate = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3])  # bytes/sec server->client
def pipe(src, dst, rate):
    try:
        while True:
            d = src.recv(512 if rate else 65536)
            if not d: break
            dst.sendall(d)
            if rate: time.sleep(len(d) / rate)
    except OSError: pass
    finally:
        for x in (src, dst):
            try: x.shutdown(socket.SHUT_RDWR)
            except OSError: pass
ls = socket.socket(); ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); ls.bind(("127.0.0.1", listen)); ls.listen(8)
while True:
    c, _ = ls.accept(); u = socket.create_connection(("127.0.0.1", target))
    threading.Thread(target=pipe, args=(u, c, rate), daemon=True).start(); threading.Thread(target=pipe, args=(c, u, 0), daemon=True).start()
