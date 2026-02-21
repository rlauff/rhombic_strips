import sys
import itertools

def get_diagonals(n):
    """Generiert alle möglichen gültigen Diagonalen eines n-Ecks."""
    diags = []
    for i in range(1, n + 1):
        for j in range(i + 2, n + 1):
            if i == 1 and j == n:
                continue
            diags.append((i, j))
    return diags

def crosses(d1, d2):
    """Prüft, ob sich zwei Diagonalen im Inneren des Polygons kreuzen."""
    i, j = d1
    k, l = d2
    return (i < k < j < l) or (k < i < l < j)

def get_all_valid_sets(diags, max_size):
    """Findet rekursiv alle gültigen (nicht kreuzenden) Diagonalenmengen."""
    faces_by_size = {i: [] for i in range(1, max_size + 1)}
    
    def backtrack(start_idx, current_set):
        size = len(current_set)
        if size > 0:
            faces_by_size[size].append(tuple(current_set))
            
        if size == max_size:
            return
            
        for i in range(start_idx, len(diags)):
            d = diags[i]
            if all(not crosses(d, existing) for existing in current_set):
                current_set.append(d)
                backtrack(i + 1, current_set)
                current_set.pop()
                
    backtrack(0, [])
    return faces_by_size

def find_flip_gray_code(triangulations):
    """
    Findet einen Gray-Code (Hamiltonschen Pfad) im Flip-Graphen 
    der Triangulierungen mittels DFS und Warnsdorffs Heuristik.
    """
    if not triangulations:
        return []
        
    n_tris = len(triangulations)
    
    # 1. Adjazenzliste (Flip-Graph) aufbauen
    adj = {i: [] for i in range(n_tris)}
    for i in range(n_tris):
        for j in range(i + 1, n_tris):
            # Ein Flip bedeutet, dass sich die Triangulierungen
            # in genau einer Diagonale unterscheiden
            set_i = set(triangulations[i])
            set_j = set(triangulations[j])
            if len(set_i.intersection(set_j)) == len(set_i) - 1:
                adj[i].append(j)
                adj[j].append(i)
                
    # 2. Hamiltonschen Pfad suchen
    path = []
    visited = set()
    
    def dfs(curr):
        path.append(curr)
        visited.add(curr)
        
        if len(path) == n_tris:
            return True
            
        # Warnsdorffs Heuristik: Besuche Nachbarn mit den wenigsten unbesuchten Nachbarn zuerst
        neighbors = []
        for nxt in adj[curr]:
            if nxt not in visited:
                deg = sum(1 for nn in adj[nxt] if nn not in visited)
                neighbors.append((deg, nxt))
        neighbors.sort() # Sortiert nach Grad (deg) aufsteigend
        
        for deg, nxt in neighbors:
            if dfs(nxt):
                return True
                
        path.pop()
        visited.remove(curr)
        return False
        
    # Wir starten beim ersten Knoten
    dfs(0)
    
    return [triangulations[i] for i in path]

def main():
    if len(sys.argv) != 2:
        print("Verwendung: python generate_normal_associahedron.py <n>")
        return
        
    try:
        n = int(sys.argv[1])
    except ValueError:
        print("Fehler: n muss eine ganze Zahl sein.")
        return

    if n < 4:
        print("Fehler: n muss mindestens 4 sein (Viereck).")
        return

    diags = get_diagonals(n)
    max_diagonals = n - 3
    valid_sets = get_all_valid_sets(diags, max_diagonals)
    
    # --- 1. Flächen generieren und sortieren ---
    all_faces = []
    face_to_idx = {}
    idx = 0
    
    # Das Gitter hat Dimensionen von 0 bis (max_diagonals - 1)
    # Dimension 0 entspricht maximalen Diagonalen (max_diagonals)
    for d in range(max_diagonals):
        k = max_diagonals - d  # Anzahl der Diagonalen für diese Dimension
        
        if d == 0:
            # Dimension 0 (Knoten): Generiere in Gray-Code Reihenfolge
            faces_d = find_flip_gray_code(valid_sets.get(k, []))
        else:
            # Höhere Dimensionen: Alphabetisch / Deterministisch sortieren
            faces_d = sorted(valid_sets.get(k, []))
            
        for f in faces_d:
            face_to_idx[f] = idx
            all_faces.append((d, f))
            idx += 1

    # --- 2. Datei schreiben und Upsets/Downsets ermitteln ---
    filename = f"normal_associahedron_{n}"
    with open(filename, "w") as out:
        for idx, (d, f) in enumerate(all_faces):
            
            # Label generieren (z.B. 13|35)
            label = "|".join(f"{i}{j}" for i, j in f)
            
            # Upsets (Dimension d+1): Wir entfernen genau 1 Diagonale aus dem aktuellen Set
            upsets = []
            for sub_f in itertools.combinations(f, len(f) - 1):
                if sub_f in face_to_idx:
                    upsets.append(face_to_idx[sub_f])
                    
            # Downsets (Dimension d-1): Wir fügen genau 1 verträgliche Diagonale hinzu
            downsets = []
            for diag in diags:
                if diag not in f and all(not crosses(diag, existing) for existing in f):
                    super_f = tuple(sorted(f + (diag,)))
                    if super_f in face_to_idx:
                        downsets.append(face_to_idx[super_f])
                        
            upsets.sort()
            downsets.sort()
            
            up_str = ", ".join(map(str, upsets))
            down_str = ", ".join(map(str, downsets))
            
            out.write(f"{d}: {label}: {{{up_str}}}, {{{down_str}}}\n")
            
    print(f"Erfolg! Die Datei '{filename}' wurde generiert.")

if __name__ == '__main__':
    main()