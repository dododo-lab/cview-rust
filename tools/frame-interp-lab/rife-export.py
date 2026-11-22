#!/usr/bin/env python3
"""Practical-RIFE flownet.pkl(PyTorch zip 저장 형식)을 torch 없이 원시 float32 로 내보낸다.

사용: python3 -I rife-export.py <flownet.pkl> <출력폴더>
  출력폴더/weights.bin  : 모든 텐서를 이어 붙인 little-endian float32
  출력폴더/index.json   : {이름: {"shape": [...], "offset": float 단위 시작 위치}}, 이름은 "module." 을 뗀 것

pickle 은 임의 코드를 실행할 수 있으므로, 허용 목록에 있는 형식(OrderedDict, 텐서 재구성 함수, 저장소 형식
이름)만 풀고 나머지는 거부한다. 가중치 파일은 저장소에 넣지 않는다(라이선스: MIT, 배포는 Google Drive).
"""
import collections
import json
import pickle
import struct
import sys
import zipfile
from array import array

DTYPES = {"FloatStorage": ("f", 4), "HalfStorage": ("e", 2)}


def rebuild_tensor(storage, offset, size, stride, *_):
    return {"storage": storage, "offset": offset, "size": tuple(size), "stride": tuple(stride)}


class SafeUnpickler(pickle.Unpickler):
    def find_class(self, module, name):
        if module == "collections" and name == "OrderedDict":
            return collections.OrderedDict
        if module == "torch._utils" and name == "_rebuild_tensor_v2":
            return rebuild_tensor
        if module == "torch._utils" and name == "_rebuild_parameter":
            return lambda data, requires_grad, hooks: data
        if module == "torch" and name in DTYPES:
            return name
        raise pickle.UnpicklingError(f"허용하지 않은 형식: {module}.{name}")

    def persistent_load(self, pid):
        kind, stype, key, _location, numel = pid
        if kind != "storage" or stype not in DTYPES:
            raise pickle.UnpicklingError(f"알 수 없는 저장소: {pid!r}")
        return {"type": stype, "key": key, "numel": numel}


def main():
    src, out = sys.argv[1], sys.argv[2]
    zf = zipfile.ZipFile(src)
    names = zf.namelist()
    pkl = next(n for n in names if n.endswith("/data.pkl"))
    root = pkl[: -len("data.pkl")]
    state = SafeUnpickler(zf.open(pkl)).load()
    blobs = {}
    index = {}
    flat = array("f")
    for k, t in state.items():
        name = k.replace("module.", "", 1)
        st = t["storage"]
        code, width = DTYPES[st["type"]]
        if st["key"] not in blobs:
            raw = zf.read(f"{root}data/{st['key']}")
            a = array(code)
            a.frombytes(raw)
            if sys.byteorder != "little":
                a.byteswap()
            blobs[st["key"]] = a
        a = blobs[st["key"]]
        size = t["size"]
        n = 1
        for s in size:
            n *= s
        # 연속 배치만 받는다(PyTorch 기본). 다르면 중단.
        exp, acc = [], 1
        for s in reversed(size):
            exp.insert(0, acc)
            acc *= s
        if n > 1 and tuple(exp) != t["stride"]:
            raise SystemExit(f"연속 배치가 아님: {name} size={size} stride={t['stride']}")
        index[name] = {"shape": list(size), "offset": len(flat)}
        flat.extend(float(x) for x in a[t["offset"] : t["offset"] + n])
    with open(f"{out}/weights.bin", "wb") as f:
        f.write(flat.tobytes())
    with open(f"{out}/index.json", "w") as f:
        json.dump(index, f, indent=1)
    print(f"{len(index)}개 텐서, {len(flat)}개 값({len(flat) * 4 / 1e6:.1f} MB) → {out}")


if __name__ == "__main__":
    main()
