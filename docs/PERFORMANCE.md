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

## 4. Simulation temps réel

Le mode temps réel n'exécute plus un nombre fixe de pas par image. Chaque image
dispose d'un budget mural de **5 ms** (`LIVE_TICK_BUDGET_SECONDS`). Le pas
d'intégration live est réglable de **1 µs à 10 ms** et vaut **1 ms** par défaut
(`DEFAULT_LIVE_STEP_SECONDS`) ; les analyses complètes gardent le pas saisi
(10 µs par défaut). Un échantillon est construit après chaque pas live, et le
tampon est un anneau de **10 000 échantillons** dont la tête est libérée par
blocs de 256 : aucune allocation ni recopie par image en régime établi. La
fenêtre conservée vaut donc 10 000 × le pas live : 10 s à 1 ms, 50 ms à 5 µs.

Témoin commun : échelle RC de 60 sections (61 nœuds, 122 composants) avec une
diode, 60 images/s, 5 s simulées, profil `--release`, même machine que la
section 1. Le harnais est compilé hors dépôt ; il rejoue la politique de `main`
(800 pas de 10 µs, un échantillon par pas, 100 000 échantillons plus le tampon
décimé, `Vec::drain` en tête) puis celle de cette branche, avec un compteur
d'allocations pour le tas du tampon.

| Politique | Image moyenne / p95 / max | Ratio à 1× | Échantillons retenus | Tas du tampon | Trim |
| --- | ---: | ---: | ---: | ---: | ---: |
| Avant (`main`) | 17,4 / 22,6 / 30,7 ms | 0,48 | 100 000 | 180 Mo | 0,39 ms/image, 1,06 ms max |
| Après, pas 1 ms, 1× | 0,35 / 0,43 / 0,94 ms | 1,000 | 5 001 (5 s) | 8,9 Mo | 0 ms |
| Après, pas 1 ms, 12 s à 1× | 0,36 / 0,42 / 1,53 ms | 1,000 | 10 000 (plafond) | 18,1 Mo | 0,20 ms max |
| Après, pas 1 ms, 10× | 3,41 / 3,68 / 5,01 ms | 1,000 | 10 000 | 17,8 Mo | 0,26 ms max |

Avant, 300 images sur 300 dépassaient 6 ms et 291 dépassaient 16 ms ; après,
aucune ne les dépasse, y compris à 10×. À 1× le ratio simulé/réel passe de 0,48
à 1,000. Le budget borne l'image : à 10× le maximum mesuré (5,01 ms) reste dans
le budget plus un pas et un échantillon.

Pas fin (5 µs), même témoin, même budget :

| Réglage | Image moyenne / p95 / max | Ratio | Échantillons / fenêtre | Tas |
| --- | ---: | ---: | ---: | ---: |
| 5 µs, 1× | 5,02 / 5,04 / 5,14 ms | 0,057 | 9 819 / 49 ms | 17,4 Mo |
| 5 µs, 0,1× | 5,02 / 5,03 / 5,24 ms | 0,552 | 9 854 / 49 ms | 17,5 Mo |
| 5 µs, 0,05× | 4,74 / 5,04 / 5,22 ms | 1,000 | 9 809 / 49 ms | 17,4 Mo |

Sur un petit RC (1 kΩ, 100 µF), 5 µs tient 1× à 60 images/s (1,88 ms au pire
2,57 ms) ; à 5× le ratio tombe à 0,529. Un pas fin coûte donc des pas, pas des
images : l'interface reste fluide, le temps simulé avance moins vite. Pour
rester à 100 % du temps réel avec un pas fin, baisser la vitesse (0,05× suffit
sur le témoin à 5 µs).

Compromis assumés :

- le pas live par défaut vaut 1 ms ; un pas plus fin est un choix explicite et
  le ratio affiché montre le ralentissement. Les analyses DC/AC/transitoires
  complètes, les CSV et les spectres hors live gardent tous leurs échantillons
  au pas saisi ;
- le spectre du mode live utilise le pas effectif (Nyquist 500 Hz à 1 ms,
  100 kHz à 5 µs) ;
- la fenêtre conservée vaut 10 000 × le pas live (10 s à 1 ms, 50 ms à 5 µs) ;
  le sélecteur de fenêtre descend à 1 ms ;
- un événement de source est appliqué à la première frontière du pas live qui
  suit son instant, avec un décalage maximal d'un pas ;
- la session reste sur le fil de l'interface : le budget borne le travail par
  image, mais un pas unique n'est pas préemptible.

## 5. Précautions et baseline

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
