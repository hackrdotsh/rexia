"""Send one 40x20 image per Kitty transfer medium, each in its own color, with labels."""
import base64, os, sys

W, H = 40, 20

def rgb(r, g, b):
    return bytes([r, g, b]) * (W * H)

def send(control, payload=b""):
    data = base64.standard_b64encode(payload)
    if len(data) <= 4096:
        sys.stdout.buffer.write(b"\x1b_G" + control.encode() + b";" + data + b"\x1b\\")
    else:
        chunks = [data[i:i + 4096] for i in range(0, len(data), 4096)]
        for n, chunk in enumerate(chunks):
            more = 1 if n < len(chunks) - 1 else 0
            head = (control + "," if n == 0 else "") + f"m={more}"
            sys.stdout.buffer.write(b"\x1b_G" + head.encode() + b";" + chunk + b"\x1b\\")
    sys.stdout.flush()

common = f"f=24,s={W},v={H},c=8,r=2,q=2"
pid = os.getpid()

print("direct:")
send(f"a=T,t=d,i=11,{common}", rgb(220, 40, 40))
print("\nfile:")
path = f"/tmp/rexia-kitty-file-{pid}.rgb"
open(path, "wb").write(rgb(40, 200, 40))
send(f"a=T,t=f,i=12,{common}", path.encode())
print("\ntemp file:")
temp = f"/tmp/tty-graphics-protocol-rexia-{pid}.rgb"
open(temp, "wb").write(rgb(40, 80, 220))
send(f"a=T,t=t,i=13,{common}", temp.encode())
print("\nshared memory:")
name = f"rexia-kitty-shm-{pid}"
open(f"/dev/shm/{name}", "wb").write(rgb(220, 200, 40))
send(f"a=T,t=s,i=14,{common}", name.encode())
print("\nsent")
