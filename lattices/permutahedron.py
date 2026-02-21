import sys
from collections import defaultdict

def sjt_permutations(n):
    """
    Generiert Permutationen von 1 bis n strikt nach dem 
    Steinhaus-Johnson-Trotter-Algorithmus.
    Gibt diese als Tupel von 1-Element-Tupeln zurück (z.B. ((1,), (2,), (3,))).
    """
    if n == 0:
        return
        
    pi = list(range(1, n + 1))
    # Richtungen: -1 bedeutet links, 1 bedeutet rechts
    dirs = [-1] * n 
    
    # Erste Permutation ausgeben
    yield tuple((x,) for x in pi)
    
    while True:
        mobile_val = -1
        mobile_idx = -1
        
        # Größtes mobiles Element finden
        for i in range(n):
            d = dirs[i]
            # Zeigt nach links
            if d == -1 and i > 0 and pi[i] > pi[i-1]:
                if pi[i] > mobile_val:
                    mobile_val = pi[i]
                    mobile_idx = i
            # Zeigt nach rechts
            elif d == 1 and i < n - 1 and pi[i] > pi[i+1]:
                if pi[i] > mobile_val:
                    mobile_val = pi[i]
                    mobile_idx = i
                    
        # Wenn es kein mobiles Element mehr gibt, sind wir fertig
        if mobile_idx == -1:
            break
            
        # Swap mit dem Element, auf das es zeigt
        target_idx = mobile_idx + dirs[mobile_idx]
        pi[mobile_idx], pi[target_idx] = pi[target_idx], pi[mobile_idx]
        dirs[mobile_idx], dirs[target_idx] = dirs[target_idx], dirs[mobile_idx]
        
        # Richtung aller Elemente umkehren, die größer als das geswappte Element sind
        for i in range(n):
            if pi[i] > mobile_val:
                dirs[i] *= -1
                
        yield tuple((x,) for x in pi)

def main():
    if len(sys.argv) != 2:
        print("Verwendung: python generate_normal_permutahedron.py <n>")
        return
    
    try:
        n = int(sys.argv[1])
    except ValueError:
        print("Fehler: n muss eine ganze Zahl sein.")
        return

    if n < 1:
        print("n muss mindestens 1 sein.")
        return

    # Datenstrukturen
    all_faces = []                   # Liste aus (Dimension, Face)
    face_to_idx = {}                 # Mapping von Face-Tuple zu Index
    faces_by_dim = defaultdict(list) # Speichert Faces sortiert nach Dimension
    
    upsets = defaultdict(set)
    downsets = defaultdict(set)
    
    # 1. Dimension 0 (Knoten/Permutationen) in SJT-Reihenfolge generieren
    for face in sjt_permutations(n):
        idx = len(all_faces)
        face_to_idx[face] = idx
        all_faces.append((0, face))
        faces_by_dim[0].append(face)
        
    # 2. Höhere Dimensionen generieren (bis n-2, d.h. Facetten)
    # Dimension d entsteht durch das Verschmelzen von 2 benachbarten Blöcken aus Dimension d-1
    for d in range(1, n - 1):
        for prev_face in faces_by_dim[d - 1]:
            # Wir betrachten alle benachbarten Blöcke i und i+1
            for i in range(len(prev_face) - 1):
                # Verschmelze sie (sortiert als Set repräsentiert)
                merged_block = tuple(sorted(prev_face[i] + prev_face[i+1]))
                new_face = prev_face[:i] + (merged_block,) + prev_face[i+2:]
                
                # Neues Face registrieren, falls es noch nicht existiert
                if new_face not in face_to_idx:
                    idx = len(all_faces)
                    face_to_idx[new_face] = idx
                    all_faces.append((d, new_face))
                    faces_by_dim[d].append(new_face)
                    
                # Relationen speichern: 
                # new_face ist ein Upset von prev_face (eine Dimension höher)
                # prev_face ist ein Downset von new_face (eine Dimension tiefer)
                prev_idx = face_to_idx[prev_face]
                new_idx = face_to_idx[new_face]
                
                upsets[prev_idx].add(new_idx)
                downsets[new_idx].add(prev_idx)

    # 3. Datei im geforderten Format ausschreiben
    filename = f"normal_permutahedron_{n}"
    with open(filename, "w") as out:
        for idx, (d, f) in enumerate(all_faces):
            # Label generieren, z.B. 1|2|3 oder 12|3
            label = "|".join("".join(map(str, block)) for block in f)
            
            # Upsets und Downsets sortieren für saubere Ausgabe
            up_sorted = sorted(list(upsets[idx]))
            down_sorted = sorted(list(downsets[idx]))
            
            up_str = ", ".join(map(str, up_sorted))
            down_str = ", ".join(map(str, down_sorted))
            
            out.write(f"{d}: {label}: {{{up_str}}}, {{{down_str}}}\n")
            
    print(f"Erfolg! Die Datei '{filename}' wurde generiert.")

if __name__ == '__main__':
    main()