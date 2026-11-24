#!/usr/bin/env python3
"""VM(192.168.64.x) 전용 임시 HTTP CONNECT 프록시 — VM 다리 주소에만 연다."""
import asyncio, sys

HOST = sys.argv[1] if len(sys.argv) > 1 else "192.168.64.1"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 18888


async def pipe(r, w):
    try:
        while True:
            b = await r.read(65536)
            if not b:
                break
            w.write(b)
            await w.drain()
    except Exception:
        pass
    finally:
        try:
            w.close()
        except Exception:
            pass


async def handle(cr, cw):
    peer = cw.get_extra_info("peername")
    if not peer or not peer[0].startswith("192.168.64."):
        cw.close()
        return
    try:
        head = await cr.readuntil(b"\r\n\r\n")
    except Exception:
        cw.close()
        return
    line = head.split(b"\r\n", 1)[0].decode("latin1")
    parts = line.split()
    if len(parts) < 2:
        cw.close()
        return
    method, target = parts[0], parts[1]
    try:
        if method == "CONNECT":
            host, port = target.rsplit(":", 1)
            ur, uw = await asyncio.wait_for(asyncio.open_connection(host, int(port)), 15)
            cw.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            await cw.drain()
        else:
            # 평문 HTTP: 절대 URL 요청
            from urllib.parse import urlsplit
            u = urlsplit(target)
            ur, uw = await asyncio.wait_for(asyncio.open_connection(u.hostname, u.port or 80), 15)
            path = (u.path or "/") + (("?" + u.query) if u.query else "")
            rest = head.split(b"\r\n", 1)[1]
            uw.write(f"{method} {path} {parts[2] if len(parts) > 2 else 'HTTP/1.1'}\r\n".encode() + rest)
            await uw.drain()
    except Exception:
        cw.close()
        return
    await asyncio.gather(pipe(cr, uw), pipe(ur, cw))


async def main():
    srv = await asyncio.start_server(handle, HOST, PORT)
    print(f"proxy on {HOST}:{PORT}", flush=True)
    async with srv:
        await srv.serve_forever()


asyncio.run(main())
