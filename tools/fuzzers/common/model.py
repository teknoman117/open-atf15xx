"""Hypothesis model of the ATF15xxAS macrocell config bit layout (to be verified by oracle experiments)."""
import json, os
# fuse regions from tools/fuzzers/fuse-regions
REG = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '.scratch', 're', 'regions.json')))
FWD_1504 = {(0, 1), (4, 23), (6, 9), (7, 19), (12, 16), (18, 20)}   # slot == l on 1504/1508; reversed on 1502


def norm(p):
    return tuple(sorted(p))


def fields(fam):
    return {(f['block'], norm(f['bits'])): f for f in REG[fam + 'AS']['mcfg'] if f['bits']}


def slot(fam, pair, l):
    fwd = (norm(pair) in FWD_1504) != (fam == '1502')
    return l if fwd else 15 - l


def addr(fam, mc, pair, k, F=None):
    F = F or fields(fam)
    blk = 'ABCDEFGH'[(mc - 1) // 16]; l = (mc - 1) % 16
    f = F[(blk, norm(pair))]
    s = slot(fam, pair, l)
    return f['addr'] + 2 * s + (k ^ (s & 1))


PAIRS = sorted({norm(f['bits']) for f in REG['1504AS']['mcfg'] if f['bits']})


def mcbits(fam, mc, fuses, base=0, F=None):
    F = F or fields(fam)
    return ''.join(fuses[addr(fam, mc, p, k, F) - base] for p in PAIRS for k in (0, 1))
