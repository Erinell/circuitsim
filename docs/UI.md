# Interface egui

`circuitsim-app` est l'application native et WASM. Elle édite le schéma, affiche
l'ERC et pilote les instruments. Les calculs restent dans `sim-core` et les
algorithmes de mesure/FFT/Bode/export sont dans [INSTRUMENTS.md](INSTRUMENTS.md).
La distribution web utilise `index.html` et Trunk.

## 1. Modules et disposition

| Module | Rôle |
| --- | --- |
| `app.rs` | état racine, panneaux, barre d'outils, raccourcis, aide |
| `document.rs` | schéma, historique, sélection, cache d'extraction |
| `view.rs` | transformations grille/écran, zoom, ajustement |
| `analysis.rs` | catalogue, préparation worker, traces, mesures, FFT, Bode, CSV |
| `export.rs` | fichier natif et téléchargement navigateur |
| `ui/canvas.rs` | entrées souris et peinture |
| `ui/palette.rs`, `ui/glyph.rs` | outils et symboles |
| `ui/properties.rs`, `ui/erc.rs` | inspecteur, ERC et nets |
| `ui/instruments.rs` | panneau de simulation et graphiques |

De haut en bas : barre d'outils (document, undo/redo, outils, transformations,
Instruments, aide), palette filtrable à gauche, canevas central, inspecteur/ERC
à droite, instruments en bas (210–560 px), barre d'état. Les onglets **Portée**,
**Spectre** et **Bode** conservent leurs résultats séparément. La démonstration
présélectionne `V1`, `5 V` et `OUT`.

Le canevas s'ajuste au contenu au démarrage et via **Ajuster**/`Home`. La grille
est entière avec un pas visuel adaptatif.

## 2. Interactions

- Clic : sélectionne l'objet sous le curseur ou vide la sélection.
- `Maj` + clic : ajoute/retire un objet.
- Glisser : déplace la sélection d'un delta entier.
- `Ctrl+A` : tout sélectionner ; `Échap` : annuler l'outil ou la sélection.
- Molette : zoom centré sous le curseur (8–80 px/case).
- Bouton milieu + glisser : déplacer la vue ; `Home` : ajuster.

Priorité au clic : label, jonction, symbole, fil. Le survol affiche une
prévisualisation pour le placement, le fil, la jonction, le label et le
déplacement.

Le placement utilise la grille avec un label unique. L'outil Fil demande deux
clics ; un tracé diagonal devient deux segments orthogonaux avec coin en
`(x arrivée, y départ)`. Une extrémité identique ou un fil existant est refusé
sans consommer d'identifiant. Jonction refuse les doublons ; Label ouvre un
dialogue validé avant insertion.

## 3. Propriétés et ERC

L'inspecteur se synchronise sur `(révision, sélection)` et travaille sur un
brouillon jusqu'à **Appliquer**.

- Symbole : référence, orientation, miroir, paramètres typés.
- Label : nom du net ; fil/jonction : géométrie en lecture seule.
- Sélection multiple : compteurs et actions pivoter, miroir, supprimer.

Les nombres sont formatés en ingénierie et reparsés avant validation. Une valeur
invalide, un nom vide ou une transformation hors grille laisse document,
révision et compteur d'identifiants inchangés. Les messages utilisateur sont en
français ; les erreurs techniques restent en anglais.

L'extraction est recalculée après chaque commande acceptée. Le panneau ERC
affiche les compteurs et les diagnostics triés ; un clic sélectionne l'objet
correspondant s'il existe. L'ERC est un rapport : seule une erreur structurelle
d'extraction bloque les nets et les analyses.

## 4. Instruments

Les contrôles acceptent `5m`, `10u`, etc. Le bouton de lancement est désactivé
sans extraction valide, pendant une analyse ou si le worker est indisponible.
Les résultats conservent la révision et les noms du document ; un résultat
antérieur est signalé mais reste consultable.

- **Portée** : champs Début/Arrêt/Pas, échelon de source (initiale, instant,
  finale), signaux avec boutons **Toutes tensions**/**Aucun**. Les courbes
  suivent l'ordre trié. Mesures min/max, crête-à-crête, moyenne, RMS et nombre
  d'échantillons.
- **Spectre** : réutilise le dernier transitoire, choisit un signal et affiche
  pic non continu, fréquence et résolution.
- **Bode** : analyse AC logarithmique, source, amplitude, sortie ; gain et phase
  partagent l'axe des fréquences.
- **Export CSV** : copie ou enregistre/télécharge toutes les valeurs de l'onglet
  actif ; le nom proposé dépend de l'onglet.

Un échec de validation, refus du worker ou échec de solveur est affiché en rouge
et ne remplace pas le dernier résultat valide. Pendant l'attente, les boutons sont
désactivés et le panneau se rafraîchit toutes les 20 ms. Les détails de calcul et
les formats de fichiers sont dans [INSTRUMENTS.md](INSTRUMENTS.md).

## 5. Raccourcis

| Touche | Action |
| --- | --- |
| `V`, `W`, `J`, `L` | Sélection, Fil, Jonction, Label |
| `R`, `M` | pivoter, miroir |
| `Suppr`, `Retour arrière` | supprimer |
| `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Maj+Z` | annuler, rétablir |
| `Ctrl+A` | tout sélectionner |
| `Échap` | annuler l'outil ou la sélection |
| `Home` | ajuster la vue |

Les raccourcis sont ignorés si un champ texte a le focus. Aucun raccourci global
ne lance une simulation.

## 6. Atomicité, limites et tests

Toute création alloue l'identifiant sur un clone et n'exécute la commande
qu'après validation. Une action refusée ne change ni contenu, ni révision, ni
`next_id`. L'undo restaure les objets complets mais pas le compteur
d'identifiants. Collections et rendu sont ordonnés.

Limites : worker dans le fil de l'interface, tests navigateur non automatisés,
transitoire à pas fixe, FFT plafonnée, pas de curseurs/PNG/persistance de projet,
opamp sans alimentation/pôle, pas de thème clair ni de mobile spécifique, fils
non extensibles par extrémité.

Tests : `cargo test -p circuitsim-app` couvre document, vue, édition, palette,
propriétés, ERC, canevas headless, instruments, worker, FFT, Bode, CSV et export.
Détails dans [TESTING.md](TESTING.md) ; recette web dans
[DELIVERY.md](DELIVERY.md).
