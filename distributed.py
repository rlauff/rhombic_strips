import sys
import itertools as it

# ==========================================
# 1. POSET GENERATION
# ==========================================

def create_fence_poset(n):
    poset = {}
    for i in range(n):
        if i % 2 == 0:
            poset[i] = ()
        else:
            covers = [i - 1]
            if i + 1 < n:
                covers.append(i + 1)
            poset[i] = tuple(covers)
    return poset

# ==========================================
# 2. LATTICE GENERATION
# ==========================================

def is_ideal(subset, poset):
    """Checks if a given subset is an order ideal."""
    for x in subset:
        if not all(y in subset for y in poset[x]):
            return False
    return True

def distributive_lattice(poset):
    """
    Generates the distributive lattice.
    Returns two dictionaries: lattice_down (covers below) and lattice_up (covers above).
    """
    elements = list(poset.keys())
    ideals = set()
    
    for r in range(len(elements) + 1):
        for combo in it.combinations(elements, r):
            subset = frozenset(combo) 
            if is_ideal(subset, poset):
                ideals.add(subset)
                
    lattice_down = {}
    lattice_up = {I: [] for I in ideals}
    
    for J in ideals:
        below_J = []
        for I in ideals:
            if I.issubset(J) and len(I) == len(J) - 1:
                below_J.append(I)
                # If I is directly below J, then J is directly above I
                lattice_up[I].append(J) 
                
        lattice_down[J] = tuple(below_J)
        
    return lattice_down, lattice_up

# ==========================================
# 3. FILE EXPORT & EXECUTION
# ==========================================

def main():
    # 1. Parse command line argument safely
    if len(sys.argv) != 2:
        sys.exit("Usage: python fence_generator.py <n>")
    try:
        n = int(sys.argv[1])
    except ValueError:
        sys.exit("Error: <n> must be a valid integer.")
        
    # 2. Generate structures
    poset = create_fence_poset(n)
    lattice_down, lattice_up = distributive_lattice(poset)
    
    # 3. Sort ideals deterministically to assign fixed line numbers
    # We sort by Rank (length) first, then by the sorted elements inside the ideal
    sorted_ideals = sorted(list(lattice_down.keys()), key=lambda I: (len(I), sorted(I)))
    
    # Create a mapping dictionary: ideal -> line_index (0-indexed)
    ideal_to_index = {ideal: idx for idx, ideal in enumerate(sorted_ideals)}
    
    # 4. Write to file
    filename = f"fence_distributed_{n}"
    with open(filename, "w") as f:
        for ideal in sorted_ideals:
            rank = len(ideal)
            
            # Format label: sort elements so it always looks consistent
            sorted_elements = sorted(ideal)
            label_str = "temp"
            
            # Get up and down indices and sort them for neatness
            up_indices = sorted([ideal_to_index[up_I] for up_I in lattice_up[ideal]])
            down_indices = sorted([ideal_to_index[down_I] for down_I in lattice_down[ideal]])
            
            # Format {up} and {down} strings
            up_str = "{" + ", ".join(map(str, up_indices)) + "}"
            down_str = "{" + ", ".join(map(str, down_indices)) + "}"
            
            # Write the exact required line format to the file
            f.write(f"{rank}: {label_str}: {up_str}, {down_str}\n")

if __name__ == "__main__":
    main()