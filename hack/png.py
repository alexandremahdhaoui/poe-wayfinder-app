import zlib, struct

def decode(path):
    d = open(path, 'rb').read()
    i = 8; idat = b''; w = h = ct = 0
    while i < len(d):
        ln = struct.unpack('>I', d[i:i+4])[0]
        typ = d[i+4:i+8]; data = d[i+8:i+8+ln]
        if typ == b'IHDR': w, h, _, ct = struct.unpack('>IIBB', data[:10])
        elif typ == b'IDAT': idat += data
        i += 12 + ln
    raw = zlib.decompress(idat)
    ch = {0:1, 2:3, 3:1, 4:2, 6:4}[ct]
    stride = w * ch
    out = bytearray(); prev = bytearray(stride); pos = 0
    for _ in range(h):
        f = raw[pos]; pos += 1
        line = bytearray(raw[pos:pos+stride]); pos += stride
        if f:
            for x in range(stride):
                a = line[x-ch] if x >= ch else 0
                b = prev[x]
                c = prev[x-ch] if x >= ch else 0
                if f == 1: line[x] = (line[x] + a) & 255
                elif f == 2: line[x] = (line[x] + b) & 255
                elif f == 3: line[x] = (line[x] + (a+b)//2) & 255
                else:
                    p = a + b - c; pa = abs(p-a); pb = abs(p-b); pc = abs(p-c)
                    pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                    line[x] = (line[x] + pr) & 255
        out += line; prev = line
    if ch == 3:
        rgba = bytearray()
        for i in range(0, len(out), 3):
            rgba += out[i:i+3] + b'\xff'
        out = rgba
    return w, h, bytearray(out)

def encode(path, w, h, px):
    raw = bytearray()
    stride = w * 4
    for y in range(h):
        raw.append(0)
        raw += px[y*stride:(y+1)*stride]
    def chunk(typ, data):
        c = struct.pack('>I', len(data)) + typ + data
        return c + struct.pack('>I', zlib.crc32(typ + data) & 0xffffffff)
    png = b'\x89PNG\r\n\x1a\n'
    png += chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(bytes(raw), 9))
    png += chunk(b'IEND', b'')
    open(path, 'wb').write(png)

def resize(w, h, px, nw, nh):
    out = bytearray(nw * nh * 4)
    for y in range(nh):
        y0 = y * h // nh; y1 = max(y0 + 1, (y+1) * h // nh)
        for x in range(nw):
            x0 = x * w // nw; x1 = max(x0 + 1, (x+1) * w // nw)
            acc = [0, 0, 0, 0]; n = 0
            for sy in range(y0, y1):
                base = sy * w * 4
                for sx in range(x0, x1):
                    o = base + sx * 4
                    a = px[o+3]
                    acc[0] += px[o] * a; acc[1] += px[o+1] * a; acc[2] += px[o+2] * a
                    acc[3] += a; n += 1
            o = (y * nw + x) * 4
            if acc[3]:
                out[o] = min(255, acc[0] // acc[3])
                out[o+1] = min(255, acc[1] // acc[3])
                out[o+2] = min(255, acc[2] // acc[3])
            out[o+3] = acc[3] // n
    return out

def over(w, h, px, bg):
    out = bytearray(w * h * 4)
    for i in range(0, len(px), 4):
        a = px[i+3] / 255
        for c in range(3):
            out[i+c] = int(px[i+c] * a + bg[c] * (1 - a))
        out[i+3] = 255
    return out
