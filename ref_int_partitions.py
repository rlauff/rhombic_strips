import sys

def partitions(n):
    """Generiert alle Partitionen der Zahl n in absteigender Reihenfolge."""
    def p_helper(n, max_val):
        if n == 0:
            yield []
            return
        # Iteriere rückwärts, um absteigend sortierte Listen zu erhalten
        for x in range(min(n, max_val), 0, -1):
            for p in p_helper(n - x, x):
                yield [x] + p
                
    yield from p_helper(n, n)

def p_to_str(p):
    """Konvertiert die Partition für das Label in einen String."""
    s = ""
    for x in p:
        s += str(x)
    return s

def is_above(p1, p2):
    """
    Prüft, ob p1 ein direkter Refinement (Verfeinerung) von p2 ist.
    p1 steht direkt über p2, wenn p1 genau ein Element mehr hat
    und sich p2 bilden lässt, indem zwei Elemente aus p1 addiert werden.
    """
    if len(p1) != len(p2) + 1:
        return False
    
    # Prüfe alle Paare in p1
    for i in range(len(p1)):
        for j in range(i + 1, len(p1)):
            combined = p1[i] + p1[j]
            # Erstelle eine neue Partition, indem p1[i] und p1[j] durch ihre Summe ersetzt werden
            new_p = p1[:i] + p1[i+1:j] + p1[j+1:] + [combined]
            new_p.sort(reverse=True)
            
            if new_p == p2:
                return True
                
    return False

if __name__ == "__main__":
    # Überprüfen, ob das Argument übergeben wurde
    if len(sys.argv) < 2:
        print("Verwendung: python script.py <N>")
        sys.exit(1)

    N = int(sys.argv[1])
    
    # Generiere alle Partitionen für genau N und speichere sie in einer Liste
    P = list(partitions(N))

    # Datei schreiben
    filename = f"int_partitions_ref_{N}"
    with open(filename, "w") as out:
        for i in range(len(P)):
            p = P[i]
            rank = len(p) - 1  # Der Rang skaliert mit der Verfeinerung
            label = p_to_str(p)
            
            # Finde die Indizes der Partitionen, die über bzw. unter der aktuellen liegen
            upset = str([j for j in range(len(P)) if is_above(P[j], p)])[1:-1]
            downset = str([j for j in range(len(P)) if is_above(p, P[j])])[1:-1]
            
            # Ausgabe im gewünschten Format schreiben
            out.write(f"{rank}: {label}: {{{upset}}}, {{{downset}}}\n")