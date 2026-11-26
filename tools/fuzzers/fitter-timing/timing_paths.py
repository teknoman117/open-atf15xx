"""Check that the datasheet timing-model parameters compose to the datasheet pin-to-pin specs (external).

Uses the ATF1508AS(L) datasheet values (all five grades, fractional ns as printed; transcribed
below from Table "AC Characteristics" of Atmel-0784 rev. 2014) and the path equations of
docs/architecture/fitter/08-timing.md §F8.5.
"""
G = ['-7', '-10', '-15', '-20', '-25']
P = {  # ATF1508AS(L) datasheet, internal parameters
    'tIN': [0.5, 0.5, 2, 2, 2], 'tIO': [0.5, 0.5, 2, 2, 2], 'tFIN': [1, 1, 2, 2, 2],
    'tSEXP': [4, 5, 8, 10, 12], 'tPEXP': [0.8, 0.8, 1, 1, 1.2], 'tLAD': [3, 5, 6, 7, 8],
    'tLAC': [3, 5, 6, 7, 8], 'tIOE': [2, 2, 3, 3, 4], 'tOD1': [2, 1.5, 4, 5, 6],
    'tOD3': [5, 5.5, 8, 10, 12], 'tZX1': [4, 5, 7, 9, 10], 'tXZ': [4, 5, 6, 7, 8],
    'tSU': [3, 2, 4, 5, 6], 'tH': [2, 3, 4, 5, 6], 'tRD': [1, 2, 1, 2, 2], 'tCOMB': [1, 2, 1, 2, 2],
    'tIC': [3, 5, 6, 7, 8], 'tEN': [3, 5, 6, 7, 8], 'tGLOB': [1, 1, 1, 1, 1], 'tPRE': [2, 3, 4, 5, 6],
    'tCLR': [2, 3, 4, 5, 6], 'tUIM': [1, 1, 2, 2, 2], 'tRPA': [10, 11, 13, 14, 15],
}
SPEC = {  # ATF1508AS(L) datasheet, external (pin-to-pin) parameters
    'tPD1': [7.5, 10, 15, 20, 25], 'tSU_global': [6, 7, 11, 16, 20], 'tCOP': [4.5, 5, 8, 10, 13],
    'tACOP': [7.5, 10, 15, 20, 25], 'tASU': [3, 3, 4, 4, 5], 'tCNT': [8, 10, 13, 17, 22],
}
PATHS = {
    'tPD1': 'tIN + tUIM + tLAD + tCOMB + tOD1',
    'tCOP': 'tIN + tGLOB + tRD + tOD1',
    'tSU_global': 'tIN + tUIM + tLAD + tSU - tIN - tGLOB',
    'tACOP': 'tIN + tUIM + tIC + tRD + tOD1',
    'tASU': 'tIN + tUIM + tLAD + tSU - tIN - tUIM - tIC',
    'tCNT': 'tRD + tUIM + tLAD + tSU',
}


def ev(expr, i):
    tot, sign = 0.0, 1
    for tok in expr.split():
        if tok in '+-':
            sign = 1 if tok == '+' else -1
        else:
            tot += sign * P[tok][i]
    return tot


if __name__ == '__main__':
    print('%-11s %-44s %s' % ('spec', 'path', '  '.join('%11s' % g for g in G)))
    for k, e in PATHS.items():
        cells = []
        for i in range(5):
            c, s = ev(e, i), SPEC[k][i]
            cells.append('%5g/%-5g%s' % (c, s, '=' if abs(c - s) < 1e-9 else '<' if c < s else '>'))
        print('%-11s %-44s %s' % (k, e, ' '.join(cells)))
    print('cells: composed/datasheet, "=" exact, "<" datasheet spec has margin over the sum')
