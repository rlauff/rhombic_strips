def partitions(m, M):
    if m == 0:
        yield []
        return
    if M == 0:
        yield []
        return
    # m is max value
    # M is max sum
    for x in range(1, min(m+1, M+1)):
        for p in partitions(x, M-x):
            yield [x] + p

def p_to_str(p):
    s = ""
    for x in p:
        s += str(x)
    return s

def is_above(p1, p2):
    if sum(p1) != sum(p2)+1: return False
    if len(p1) < len(p2) or len(p1) > len(p2)+1: return False
    if len(p1) == len(p2)+1: return p1[:-1] == p2
    b = False
    for i in range(len(p2)):
        if p1[i] != p2[i]:
            if b: return False
            b = True
    return True

import sys
N = int(sys.argv[1])

P = []
for n in range(2, N):
    P.extend(partitions(10000, n))

with open(f"int_partitions_{N}", "w") as out:
    for i in range(len(P)):
        p = P[i]
        rank = sum(p)-2
        label = p_to_str(p)
        upset = str([j for j in range(len(P)) if is_above(P[j], p)])[1:-1]
        downset = str([j for j in range(len(P)) if is_above(p, P[j])])[1:-1]
        out.write(f"{rank}: {label}: {{{upset}}}, {{{downset}}}\n")





