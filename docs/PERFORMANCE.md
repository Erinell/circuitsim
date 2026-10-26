# Performances

Les benchmarks donnent des repères reproductibles pour les solveurs,
l'extraction, la FFT et le CSV. Ils ne sont pas des seuils CI bloquants.

## 1. Environnement et commandes

Mesures du 26 octobre 2026 : AMD Ryzen 7 7700 (8 cœurs/16 threads), Windows,
rustc 1.99.1, profil `cargo bench` release sans LTO explicite, Criterion 0.8.2
en mode `--quick --noplot`. Les chiffres dépendent de la machine.

```text
cargo bench -p sim-core --bench solvers
cargo bench -p editor-core --bench extraction
cargo bench -p circuitsim-app --bench instruments
```

La CI exécute les trois benchmarks en rapide et sans seuil.

## 2. Charges et résultats

| Groupe | Charge | Temps |
| --- | --- | ---: |
| `dc/resistive_divider` | source 10 V, deux résistances | 1,018 µs |
| `transient/rc_501_samples` | RC 1 kΩ/1 µF, 5 ms, pas 10 µs, échelon 5 V | 315,30 µs |
| `ac/rc_101_points` | RC 1 Hz–1 MHz, 101 points | 31,817 µs |
| `extraction/16` | 16 résistances + 16 fils | 26,558 µs |
| `extraction/64` | 64 résistances + 64 fils | 121,68 µs |
| `extraction/256` | 256 résistances + 256 fils | 823,41 µs |
| `fft/16384` | trace 16 385 points, FFT 16 384, Hann | 854,16 µs |
| `csv/transient` | 2 signaux × 10 000 points | 6,955 ms |
| `csv/bode` | 101 points | 60,517 µs |

Le transitoire coûte environ 0,63 µs par pas et l'AC 0,315 µs par fréquence, DC
compris. L'extraction croît plus vite que le nombre de composants (×4,58 de 16 à
64, ×6,77 de 64 à 256), cohérent avec `O(P·W + W²)`. La FFT reste sous la
milliseconde ; le CSV de 20 000 lignes, généré uniquement au clic, coûte ~7 ms.

## 3. Budgets de revue

Marge d'au moins cinq fois la mesure rapide pour absorber la variabilité machine :

| Opération | Budget |
| --- | ---: |
| DC résistif | 10 µs |
| Transitoire 501 échantillons | 2 ms |
| AC 101 points | 250 µs |
| Extraction 256 résistances | 5 ms |
| FFT 16 384 points | 5 ms |
| CSV 2 × 10 000 points | 35 ms |

Un dépassement déclenche une revue du changement et de la machine. Criterion
n'étant pas fiable comme test bloquant sur un runner partagé, les budgets ne sont
pas automatisés.

## 4. Précautions et baseline

- `--quick` réduit l'échantillonnage et ne compare pas à une baseline.
- Les mesures natives ne préjugent pas des performances WASM.
- Les minimiseurs et tailles de matrices ne sont pas mesurés séparément.
- Le CSV est construit en mémoire ; les très gros exports dépendent de
  l'allocation et du téléchargement.

Comparaison locale :

```text
cargo bench -p sim-core --bench solvers -- --save-baseline avant
# modifier le code
cargo bench -p sim-core --bench solvers -- --baseline avant
```

Une baseline par plateforme et des seuils outillés restent à mettre en place.
