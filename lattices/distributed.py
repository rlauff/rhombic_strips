import sys
import itertools as it

# ==========================================
# 1. POSET GENERATION
# ==========================================

def create_fence_poset(n):
    """
    Creates a fence poset of size n.
    Even indices (0, 2, ...) are bottom elements.
    Odd indices (1, 3, ...) are top elements covering neighbors.
    """
    poset = {}
    for i in range(n):
        if i % 2 == 0:
            # Bottom element (covers nothing)
            poset[i] = ()
        else:
            # Top element (covers neighbors i-1 and i+1 if exists)
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
        # For an ideal, if x is in subset, everything x covers must be in subset
        if not all(y in subset for y in poset[x]):
            return False
    return True

def distributive_lattice(poset):
    """
    Generates the distributive lattice J(P).
    Returns two dictionaries: lattice_down (covers below) and lattice_up (covers above).
    """
    elements = list(poset.keys())
    ideals = set()
    
    # Generate all possible subsets and check if they are ideals
    for r in range(len(elements) + 1):
        for combo in it.combinations(elements, r):
            subset = frozenset(combo) 
            if is_ideal(subset, poset):
                ideals.add(subset)
                
    lattice_down = {}
    lattice_up = {I: [] for I in ideals}
    
    # Determine covering relations in the lattice (inclusion)
    for J in ideals:
        below_J = []
        for I in ideals:
            # I is covered by J if I is a subset of J and size differs by exactly 1
            if I.issubset(J) and len(I) == len(J) - 1:
                below_J.append(I)
                # If I is directly below J, then J is directly above I
                lattice_up[I].append(J) 
                
        lattice_down[J] = tuple(below_J)
        
    return lattice_down, lattice_up

# ==========================================
# 3. HELPER FUNCTIONS
# ==========================================

def get_element_label(i):
    """
    Converts a poset index to its label string.
    Even indices (0, 2...) -> b1, b2... (Bottom)
    Odd indices (1, 3...)  -> a1, a2... (Top)
    """
    if i % 2 == 0:
        # Example: 0->b1, 2->b2, 4->b3
        return f"b{i // 2 + 1}"
    else:
        # Example: 1->a1, 3->a2, 5->a3
        return f"a{(i - 1) // 2 + 1}"

# ==========================================
# 4. FILE EXPORT & EXECUTION
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
    # We sort by Rank (length) first, then by the sorted elements inside the ideal (as tuple)
    sorted_ideals = sorted(list(lattice_down.keys()), key=lambda I: (len(I), sorted(I)))
    
    # Create a mapping dictionary: ideal -> line_index (0-indexed)
    ideal_to_index = {ideal: idx for idx, ideal in enumerate(sorted_ideals)}
    
    # 4. Write to file
    filename = f"fence_distributed_{n}"
    with open(filename, "w") as f:
        for ideal in sorted_ideals:
            rank = len(ideal)
            
            # Convert indices in the ideal to labels (a1, b1, etc.)
            labels = [get_element_label(x) for x in ideal]
            
            # Sort labels alphabetically (a's before b's) and join without whitespace
            # Example: {0, 1} -> ['b1', 'a1'] -> sorted ['a1', 'b1'] -> "a1b1"
            labels.sort()
            label_str = "".join(labels)
            
            # Handle empty set case explicitly if needed, though "" is technically correct
            if not label_str:
                label_str = "empty" # Optional: Change to "" if you prefer strict concatenation
            
            # Get up and down indices and sort them for neatness
            up_indices = sorted([ideal_to_index[up_I] for up_I in lattice_up[ideal]])
            down_indices = sorted([ideal_to_index[down_I] for down_I in lattice_down[ideal]])
            
            # Format {up} and {down} strings
            up_str = "{" + ", ".join(map(str, up_indices)) + "}"
            down_str = "{" + ", ".join(map(str, down_indices)) + "}"
            
            # Write the exact required line format to the file
            f.write(f"{rank}: {label_str}: {up_str}, {down_str}\n")
    
    print(f"Successfully generated '{filename}' with {len(sorted_ideals)} elements.")

if __name__ == "__main__":
    main()