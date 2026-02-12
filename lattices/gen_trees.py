# -*- coding: utf-8 -*-
import sys
from collections import Counter
from itertools import product, combinations_with_replacement

# Memoization-Caches, um bereits berechnete Ergebnisse zu speichern.
# Das beschleunigt den Prozess erheblich.
PARTITION_MEMO = {}
TREE_MEMO = {}

def get_partitions(n):
    """
    Berechnet alle ganzzahligen Partitionen einer Zahl n.
    Verwendet Memoization, um wiederholte Berechnungen zu vermeiden.
    Eine Partition von n ist eine Möglichkeit, n als Summe positiver ganzer Zahlen zu schreiben.
    Beispiel: get_partitions(3) -> [[3], [2, 1], [1, 1, 1]]
    """
    if n in PARTITION_MEMO:
        return PARTITION_MEMO[n]
    if n == 0:
        return [[]]
    if n < 0:
        return []

    partitions = []
    # Wir iterieren von n bis 1, um die Teile der Partition zu finden.
    for i in range(n, 0, -1):
        # Rekursiver Aufruf, um die restliche Summe zu partitionieren.
        for sub_partition in get_partitions(n - i):
            # Wir stellen sicher, dass die Teile der Partition in absteigender Reihenfolge sind,
            # um Duplikate zu vermeiden (z.B. [2,1] ist dasselbe wie [1,2]).
            if not sub_partition or i >= sub_partition[0]:
                partitions.append([i] + sub_partition)

    PARTITION_MEMO[n] = partitions
    return partitions

def generate_rooted_trees(n):
    """
    Generiert alle nicht-isomorphen verwurzelten Bäume mit n Knoten.
    Die Wurzel ist immer der Knoten 0.
    Verwendet einen rekursiven Ansatz mit Memoization.
    """
    # Überprüfen, ob das Ergebnis bereits im Cache ist.
    if n in TREE_MEMO:
        return TREE_MEMO[n]

    # Basisfall: Ein Baum mit einem Knoten hat keine Kanten.
    if n == 1:
        return [[]] # Eine Liste, die einen Baum (leere Kantenliste) enthält.

    all_new_trees = []
    # Wir benötigen die Partitionen von n-1, um die Größen der Unterbäume zu bestimmen.
    partitions_of_n_minus_1 = get_partitions(n - 1)

    for partition in partitions_of_n_minus_1:
        # partition ist eine Liste von Größen, z.B. [2, 1] für n=4.

        # Wir gruppieren die Teile der Partition, falls einige Größen mehrfach vorkommen.
        # Beispiel: [1, 1, 1] -> Counter({1: 3})
        # Beispiel: [2, 2, 1] -> Counter({2: 2, 1: 1})
        part_counts = Counter(partition)
        
        # Liste, die die möglichen Baumkombinationen für jeden Teil der Partition speichert.
        # z.B. für [2, 2, 1]: [[(T2a, T2a), (T2a, T2b), ...], [(T1a,)]]
        tree_selections_for_parts = []

        for size, count in part_counts.items():
            # Hole alle möglichen Bäume der benötigten Größe.
            sub_trees = generate_rooted_trees(size)
            # Wähle 'count' Bäume aus der Liste der 'sub_trees' aus.
            # combinations_with_replacement erlaubt uns, denselben Baumtyp mehrmals zu wählen.
            # z.B. zwei identische Bäume der Größe 2.
            selections = list(combinations_with_replacement(sub_trees, count))
            tree_selections_for_parts.append(selections)

        # Nun erstellen wir das kartesische Produkt aller ausgewählten Baumkombinationen.
        # Das gibt uns jede mögliche Kombination von Unterbäumen für die aktuelle Partition.
        for tree_combination_tuple in product(*tree_selections_for_parts):
            # tree_combination_tuple ist z.B. (((T2a, T2b),), ((T1a,),))
            # Wir flachen diese Struktur für die einfache Iteration ab.
            subtrees_to_combine = [tree for group in tree_combination_tuple for tree in group]

            # Jetzt bauen wir den neuen Baum zusammen.
            new_tree_edges = []
            current_vertex_label = 1 # Starten der Nummerierung für die Knoten der Unterbäume.

            for subtree_edges in subtrees_to_combine:
                # Größe des aktuellen Unterbaums.
                subtree_size = len(subtree_edges) + 1
                
                # 1. Verbinde die neue Wurzel 0 mit der Wurzel des Unterbaums.
                #    Die Wurzel des Unterbaums bekommt das 'current_vertex_label'.
                new_tree_edges.append((0, current_vertex_label))

                # 2. Füge die Kanten des Unterbaums hinzu, aber mit verschobenen Knoten-Labels.
                #    Wenn eine Kante (u, v) im Unterbaum war, wird sie zu
                #    (u + current_vertex_label, v + current_vertex_label) im neuen Baum.
                for u, v in subtree_edges:
                    new_tree_edges.append((u + current_vertex_label, v + current_vertex_label))
                
                # Aktualisiere das Label für den nächsten Unterbaum.
                current_vertex_label += subtree_size

            all_new_trees.append(sorted(new_tree_edges))

    # Speichere das Ergebnis im Cache und gib es zurück.
    TREE_MEMO[n] = all_new_trees
    return all_new_trees

def main():
    """
    Hauptfunktion: Verarbeitet Kommandozeilenargumente, ruft die Generierung
    auf und schreibt das Ergebnis in eine Datei.
    """
    # Überprüfen der Kommandozeilenargumente
    if len(sys.argv) != 2:
        print("Verwendung: python script_name.py <n>")
        print("Dabei ist <n> eine positive ganze Zahl für die Anzahl der Knoten im Baum.")
        sys.exit(1)

    try:
        n = int(sys.argv[1])
        if n <= 0:
            raise ValueError()
    except ValueError:
        print("Fehler: Bitte gib eine positive ganze Zahl für n an.")
        sys.exit(1)

    print(f"Generiere alle verwurzelten Bäume mit {n} Knoten...")
    
    # Der erste Aufruf für n=1 muss global im Cache sein.
    TREE_MEMO[1] = [[]]
    
    # Generiere die Bäume
    trees = generate_rooted_trees(n)
    
    # Schreibe das Ergebnis in eine Datei
    filename = f"trees_{n}.txt"
    try:
        with open(filename, "w") as f:
            for tree in trees:
                f.write(str(tree) + "\n")
        print(f"Erfolgreich! {len(trees)} Bäume wurden in die Datei '{filename}' geschrieben.")
    except IOError as e:
        print(f"Fehler beim Schreiben der Datei: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
