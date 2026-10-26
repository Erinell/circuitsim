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
| `view.rs` | transformations grille/écran, zoom, ajustement, mini-carte |
| `analysis.rs` | catalogue, préparation worker, traces, mesures, FFT, Bode, CSV |
| `export.rs` | fichier natif et téléchargement navigateur |
| `ui/canvas.rs` | entrées souris, placement, câblage et peinture |
| `ui/command_palette.rs` | recherche floue et actions clavier |
| `ui/history.rs` | historique nommé et sauts d’état |
| `ui/palette.rs`, `ui/glyph.rs` | composants, favoris/récents et symboles |
| `ui/properties.rs`, `ui/erc.rs` | inspecteur, ERC et nets |
| `ui/instruments.rs` | panneau de simulation et graphiques |

De haut en bas : barre d'outils (document, undo/redo, outils, transformations,
alignement, **Cadrer sél.**, **Vue**, **Commandes**, aide), palette filtrable à
gauche, canevas central, inspecteur/ERC à droite, instruments en bas (210–560 px),
barre d'état. `Ctrl+K` ouvre la palette de commandes. Les onglets **Portée**,
**Spectre** et **Bode** conservent leurs résultats séparément. La démonstration
présélectionne `V1`, `5 V` et `OUT`.

Le menu **Vue** masque chaque panneau, active la mini-carte et applique une
disposition fiable : **Tout**, **Édition** (palette et inspecteur),
**Simulation** (instruments et inspecteur) ou **Canevas** seul. Ces dispositions
sont aussi accessibles par `Ctrl+1` à `Ctrl+4`. Les panneaux restent des zones
fixes redimensionnables ; aucun dockage libre n'est proposé.

Le canevas s'ajuste au contenu au démarrage et via **Ajuster**/`Home`. **Cadrer
sél.**/`Ctrl+Home` cadre la sélection. La grille est entière avec un pas visuel
adaptatif. La mini-carte du coin inférieur droit montre le contenu, la sélection
et la fenêtre visible ; un clic y recentre la vue.

## 2. Interactions

- Clic : sélectionne l'objet sous le curseur ou vide la sélection.
- `Maj` + clic : ajoute/retire un objet ; `Ctrl` + clic : inverse l'objet.
- `Alt` + clic : sélectionne le net complet.
- Rectangle gauche→droite : objets entièrement inclus ; droite→gauche : touchés.
- Glisser un symbole/label/jonction : déplace la sélection d'un delta entier.
- Glisser un fil : déplace le segment perpendiculairement.
- `Ctrl+A` : tout sélectionner ; `Échap` : annuler l'outil, le collage ou la sélection.
- `Ctrl+C`, `Ctrl+V`, `Ctrl+D` : copier, coller avec aperçu, dupliquer.
- Molette : zoom centré sous le curseur (8–80 px/case).
- `+`/`-` : zoom autour du centre ; `0` : zoom 100 %.
- `Ctrl+0` : retour à l'origine et au zoom par défaut.
- Flèches : déplacer la vue de deux cases ; `Maj` : huit cases.
- Bouton milieu + glisser : déplacer la vue ; `Home` : ajuster.
- Mini-carte : clic pour recentrer la vue sur le contenu.

Priorité au clic : label, jonction, symbole, fil. Le survol d'un fil ou d'une
broche met en évidence le net complet. Des guides apparaissent pendant le
déplacement lorsque des symboles partagent une coordonnée. Le menu contextuel
propose les actions propres à l’objet et, sur le vide, le collage et la vue.

Le placement utilise la grille avec un label unique et reste actif pour des
placements répétés. `R`/`F` orientent l’aperçu. Un composant à deux broches déposé
sur un fil est inséré en série. L'outil Fil part d'une grille, d'une broche ou
d'un point de fil ; le tracé est orthogonal, évite les symboles et `Maj` change
le premier coude. Une extrémité identique ou un fil existant est refusé sans
consommer d'identifiant. Jonction refuse les doublons ; Label propose les nets
existants.

## 3. Propriétés et ERC

L'inspecteur se synchronise sur `(révision, sélection)` et travaille sur un
brouillon jusqu'à **Appliquer** ou `Entrée`.

- Symbole : référence, orientation, miroir, paramètres typés.
- Label : nom du net ; fil/jonction : géométrie en lecture seule.
- Sélection multiple : compteurs et actions pivoter, miroir, supprimer.
- Paramètres numériques : saisie directe `4k7`/`100n`, valeur normalisée
  affichée, erreur locale, série E12/E24/E96 et cycle molette, `Alt+↑`/`Alt+↓`
  ou boutons `−`/`+`.
- Grandeurs simulées : dernier échantillon transitoire, tensions par broche et
  par net, courant et puissance lorsque calculables. Un résultat antérieur est
  signalé.
- Historique nommé repliable : chaque entrée indique son état ; un clic ramène
  le schéma à cet état, dans le passé ou le futur.

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
| `Ctrl+K` | palette de commandes |
| `V`, `W`, `J`, `L` | Sélection, Fil, Jonction, Label |
| `R`, `M` | pivoter, miroir |
| `R`, `F` | orienter/miroiter l’aperçu de placement |
| `Maj` / `Ctrl` + clic | ajouter / inverser dans la sélection |
| `Maj` pendant un fil | changer le coude |
| `Alt` + clic | sélectionner le net complet |
| `Ctrl+C`, `Ctrl+V`, `Ctrl+D` | copier, coller, dupliquer |
| `Alt+↑`, `Alt+↓` | cycler la série E du paramètre |
| `Suppr`, `Retour arrière` | supprimer |
| `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Maj+Z` | annuler, rétablir |
| `Ctrl+A` | tout sélectionner |
| `?` | aide et raccourcis |
| `Home` | ajuster la vue au schéma |
| `Ctrl+Home` | cadrer la sélection |
| `+`, `-`, `0` | zoom avant, arrière, 100 % |
| `Ctrl+0` | origine et zoom par défaut |
| Flèches, `Maj`+flèches | déplacer la vue |
| `Ctrl+1`…`Ctrl+4` | Tout, Édition, Simulation, Canevas |
| `Échap` | annuler l'outil, le collage ou la sélection |

Les raccourcis sont ignorés si un champ texte a le focus, à l’exception de
`Ctrl+K` pour la palette de commandes. Aucun raccourci global ne lance une
simulation.

## 6. Atomicité, limites et tests

Toute création alloue l'identifiant sur un clone et n'exécute la commande
qu'après validation. Une action refusée ne change ni contenu, ni révision, ni
`next_id`. L'undo restaure les objets complets mais pas le compteur
d'identifiants. Collections et rendu sont ordonnés.

Limites : worker dans le fil de l'interface, tests navigateur non automatisés,
transitoire à pas fixe, FFT plafonnée, pas de curseurs/PNG/persistance de projet,
opamp sans alimentation/pôle, pas de thème clair ni de mobile spécifique, fils
non extensibles par extrémité. Les panneaux sont masquables et redimensionnables,
mais il n'y a pas de dockage libre ni de disposition persistée. Les valeurs
simulées de l’inspecteur proviennent du dernier transitoire ; le câblage à
double-clic et la persistance des favoris restent aux lots suivants.

Tests : `cargo test -p circuitsim-app` couvre document, vue, navigation,
mini-carte, édition, palette, propriétés, historique, presse-papiers, sélection
rectangle, séries E, ERC, canevas headless, dispositions, instruments, worker,
FFT, Bode, CSV et export. Détails dans [TESTING.md](TESTING.md) ; recette web
dans [DELIVERY.md](DELIVERY.md).
