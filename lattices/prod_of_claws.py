import sys

def get_elements(a, b):
    """Generates all pairs (u, v) for the product of Claws Ca and Cb."""
    # Elements of a claw of size k are integers 0..k
    return [(u, v) for u in range(a + 1) for v in range(b + 1)]

def get_rank(p):
    """
    Calculates the rank of a pair (u, v).
    In a claw, 0 has rank 0, any other element has rank 1.
    The rank in the product is the sum of ranks.
    """
    rank_u = 1 if p[0] > 0 else 0
    rank_v = 1 if p[1] > 0 else 0
    return rank_u + rank_v

def is_above(p_high, p_low):
    """
    Checks if p_high covers p_low in the product order.
    Condition: Rank must differ by exactly 1, and relations must hold.
    """
    if get_rank(p_high) != get_rank(p_low) + 1:
        return False

    u1, v1 = p_high
    u2, v2 = p_low

    # Check if u component covers and v is identical
    # In Claw, x covers y iff y=0 and x>0.
    if v1 == v2:
        if u2 == 0 and u1 > 0:
            return True

    # Check if v component covers and u is identical
    if u1 == u2:
        if v2 == 0 and v1 > 0:
            return True

    return False

def p_to_str(p):
    """Converts pair tuple to string format used in filename and labels."""
    return f"{p[0]}{p[1]}"

def generate_poset(a, b):
    filename = f"prod_of_claws_{a}{b}"
    V = get_elements(a, b)

    with open(filename, "w") as out:
        for p in V:
            # Determine rank
            r = get_rank(p)

            # Find indices of elements that cover p (upper covers)
            up_covers = [i for i in range(len(V)) if is_above(V[i], p)]

            # Find indices of elements that p covers (lower covers)
            down_covers = [i for i in range(len(V)) if is_above(p, V[i])]

            # Format list strings to remove brackets, e.g., "1, 2, 3"
            up_str = str(up_covers)[1:-1]
            down_str = str(down_covers)[1:-1]

            # Write line: rank: label: {upper}, {lower}
            out.write(f"{r}: {p_to_str(p)}: {{{up_str}}}, {{{down_str}}}\n")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python prod_of_claws.py <a> <b>")
    else:
        a = int(sys.argv[1])
        b = int(sys.argv[2])
        generate_poset(a, b)
