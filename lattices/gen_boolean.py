from itertools import product

def grid(b):
    ranges = [range(x + 1) for x in b]
    return [list(p) for p in product(*ranges)]

def is_above(b1, b2):
    if sum(b1) != sum(b2)+1: return False
    dif = False
    for i in range(len(b1)):
        if b1[i] != b2[i]:
            if dif: return False
            dif = True
    return True

def b_to_str(b):
    s = ""
    for x in b:
        s += str(x)
    return s

def boolean(b):
    with open(f"grid_{b_to_str(b)}", "w") as out:
        V = grid(b)
        for bs in V:
            out.write(f"{sum(bs)}: {b_to_str(bs)}: {{{str([i for i in range(len(V)) if is_above(V[i],bs)])[1:-1]}}}, {{{str([i for i in range(len(V)) if is_above(bs,V[i])])[1:-1]}}}\n")

if __name__ == "__main__":
    import sys
    b = [int(x) for x in sys.argv[1:]]
    boolean(b)
