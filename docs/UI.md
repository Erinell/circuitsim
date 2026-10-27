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
| `ui/instruments.rs`, `ui/scopes.rs` | panneau unifié de simulation, portées et graphiques |

De haut en bas : barre d'outils avec menus **Fichier**, **Édition**, **Dessin**,
**Afficher**, **Options** et **Aide**, puis uniquement les contrôles
**Marche/Arrêt**, **Reset** et **Pas** ; palette filtrable à gauche, canevas
central, inspecteur/ERC à droite, panneau unifié **Instruments et portées** en
bas (210–620 px, visible par défaut), barre d'état. Les outils, composants,
transformations, alignements, exemples et commandes de vue sont dans les menus et
la palette. `Ctrl+K` ouvre la palette de commandes. Les onglets **Portées**,
**Spectre** et **Bode** partagent le même panneau. La démonstration présélectionne
`V1`, `5 V` et `OUT`.

À l'ouverture d'un schéma valide, la session temps réel démarre seule : le
circuit est déjà en marche et les instruments affichent le signal sans clic.

Le menu **Vue** masque chaque panneau, active la mini-carte et applique une
disposition fiable : **Tout**, **Édition** (palette et inspecteur),
**Simulation** (instruments et inspecteur) ou **Canevas** seul. Ces dispositions
sont aussi accessibles par `Ctrl+1` à `Ctrl+4`. Les panneaux restent des zones
fixes redimensionnables ; aucun dockage libre n'est proposé. Les boutons **«**
et **»** replient directement la palette et l’inspecteur.

Le canevas s'ajuste au contenu au démarrage et via **Ajuster**/`Home`. **Cadrer
sél.**/`Ctrl+Home` cadre la sélection. La grille est entière avec un pas visuel
adaptatif. La fenêtre native s'ouvre maximisée ; les fenêtres **Démarrage**,
**Aide** et **Accessibilité** sont centrées.

Les fils sont épaissis et colorés selon la tension du net après simulation :
dégradé bleu/gris/orange par défaut, variante cyan/gris/magenta, avec une
légende `-V / 0 / +V`. Le menu **Afficher** règle indépendamment les calques
Tension, Courant, Puissance, Valeurs, Références et Labels de nets. Les erreurs
ERC restent rouges et prioritaires. Le flux de courant utilise des points jaunes
à halo sombre, avec densité et vitesse proportionnelles au courant et un choix
entre sens conventionnel et sens des électrons ; un interrupteur ouvert coupe
l’animation. Un halo de puissance discret entoure les composants non résistifs,
avec une luminosité réglable. **Vue → Courant animé** reste un raccourci pour le
calque courant. Une LED rayonne derrière son symbole lorsque sa tension directe
dépasse son seuil, avec un halo réduit au dézoom, et un clic sur un interrupteur
l'ouvre ou le ferme.
Une extrémité libre d'un fil peut être tirée le long de son axe pour l'allonger
ou le raccourcir ; les extrémités connectées ne sont pas redimensionnables. La
mini-carte du coin inférieur droit montre le contenu, la sélection et la fenêtre
visible ; un clic y recentre la vue.

Les valeurs sont affichées en préfixes SI à côté des symboles, avec une taille
réglable et un placement qui évite les fils. Les références et les labels de
nets peuvent être masqués séparément. Le thème **Clair**, la **petite grille** et
le style de résistance **européen** ou **américain** complètent les thèmes
Sombre et Contraste élevé.

Le menu **Fichier → Exemples** charge des circuits originaux : RC, diviseur, LED,
interrupteur, RLC série et RLC parallèle commuté. Un clic droit sur un composant propose **Ajouter un
curseur** : le curseur apparaît sous le symbole, se règle par glisser ou molette
et applique la valeur à chaud en une seule étape annulable.

## 2. Interactions

- Clic : sélectionne l'objet sous le curseur ou vide la sélection ; sur un
  interrupteur, le clic bascule son état.
- Double-clic sur un symbole : ouvre la fenêtre de propriétés du composant.
- `Maj` + clic : ajoute/retire un objet ; `Ctrl` + clic : inverse l'objet.
- `Alt` + clic : sélectionne le net complet.
- Rectangle gauche→droite : objets entièrement inclus ; droite→gauche : touchés.
- Glisser un symbole/label/jonction : déplace la sélection d'un delta entier.
- Glisser une broche : crée un fil orthogonal avec accroche et aperçu, sans
  changer d'outil. Le relâchement sur une broche, un fil ou le vide termine ou
  annule proprement.
- Glisser un fil : déplace le segment perpendiculairement et coupe ses
  extrémités ; aucun fil supplémentaire n'est créé.
- Glisser une extrémité libre : allonge ou raccourcit le fil sur son axe.
- Survol : une bulle affiche le type, le nom, les paramètres, V aux broches, I
  et P. Elle reste lisible en pause et signale un résultat antérieur.
- Clic droit : propriétés, rotation, miroir, duplication, suppression, mesures,
  curseur de réglage et coupe de fil ; sur le fond, sous-menus **Ajouter** par
  catégorie.
- `Ctrl+A` : tout sélectionner ; `Échap` : annuler l'outil, le collage ou la sélection.
- `Espace` : marche/arrêt de la simulation continue ; `F5` : reset à `t = 0`.
- `Ctrl+C`, `Ctrl+V`, `Ctrl+D` : copier, coller avec aperçu, dupliquer.
- `Ctrl+Z`, `Ctrl+Y` : annuler, rétablir.
- Molette : zoom centré sous le curseur (8–80 px/case).
- `+`/`-` : zoom autour du centre ; `0` : zoom 100 %.
- `Ctrl+0` : retour à l'origine et au zoom par défaut.
- Flèches : déplacer la vue de deux cases ; `Maj` : huit cases.
- Bouton milieu + glisser : déplacer la vue ; `Home` : ajuster.
- En lecture seule (fenêtre ou dialogue ouvert), un glisser gauche sur le fond
  déplace aussi la vue sans modifier le schéma.
- Mini-carte : clic pour recentrer la vue sur le contenu.

Priorité au clic : label, jonction, symbole, fil. La zone de clic d’un symbole
épouse ses broches et son corps, sans marge de grille supplémentaire ; le cadre de
sélection reste resserré autour du dessin. Le survol d'un fil ou d'une broche met
en évidence le net complet. Des guides apparaissent pendant le déplacement
lorsque des symboles partagent une coordonnée. Le menu contextuel propose les
actions propres à l’objet et, sur le vide, le collage et la vue ; sa cible reste
celle du clic d’ouverture même si la souris se déplace.

Le placement utilise la grille avec un label unique et reste actif pour des
placements répétés. `R`/`F` orientent l’aperçu. Un composant à deux broches déposé
sur un fil est inséré en série. L'outil Fil part d'une grille, d'une broche ou
d'un point de fil ; le tracé est orthogonal, évite les symboles et `Maj` change
le premier coude. Une extrémité identique ou un fil existant est refusé sans
consommer d'identifiant. Jonction refuse les doublons ; Label propose les nets
existants.

## 3. Démarrage et accessibilité

Le bouton **Démarrage** ouvre un guide en quatre étapes (placer, câbler, simuler,
explorer). Il peut charger l’exemple RC, vider le schéma ou ouvrir l’aide. Le
guide est affiché au lancement et reste accessible ensuite ; l’état vide du
canevas renvoie aussi vers ce guide.

Le bouton **Accessibilité** ou `Ctrl+Maj+A` ouvre les préférences d’affichage :

- thème **Sombre**, **Contraste élevé** ou **Clair** ;
- taille de l’interface 85, 100, 115 ou 130 % ;
- réduction des animations ;
- rappel de la navigation clavier `Tab`/`Maj+Tab`.

Ces réglages, les calques, les palettes, la grille, les favoris, les vitesses et
la disposition des panneaux sont enregistrés automatiquement. Sur le Web, le
stockage est `localStorage` ; en natif, un fichier `preferences.json` versionné
est écrit dans le dossier de configuration. Un fichier absent, corrompu ou d’une
version future est ignoré et remplacé par les valeurs par défaut.

Le bouton **Projet** ouvre un dialogue JSON versionné : **Copier le JSON** pour
partager, **Charger ce JSON** après collage, et selon la plateforme **Lire /
Enregistrer le fichier** ou **Télécharger**. Le chargement revalide le schéma et
remplace le document, l’historique, les résultats et les portées. Le bouton
**À propos** affiche la version, la licence et les licences tierces.

## 4. Propriétés et ERC

L'inspecteur se synchronise sur `(révision, sélection)` et travaille sur un
brouillon jusqu'à **Appliquer** ou `Entrée`. Un double-clic sur un symbole ouvre
la même édition dans une fenêtre centrée, qui se ferme après application ou avec
`Échap`. Les aides de saisie sont regroupées dans les infobulles des champs ; le
panneau ne conserve que les valeurs et les actions utiles.

- Symbole : référence, orientation, miroir, paramètres typés.
- Label : nom du net ; fil/jonction : géométrie en lecture seule.
- Sélection multiple : compteurs et actions pivoter, miroir, supprimer.
- Paramètres numériques : saisie directe `4k7`/`100n`, erreur locale, série
  E12/E24/E96 et cycle molette, `Alt+↑`/`Alt+↓` ou boutons `−`/`+`. La valeur
  normalisée apparaît dans l'infobulle du champ.
- Grandeurs simulées : dernier échantillon transitoire, tensions par broche et
  par net, courant et puissance lorsque calculables. Un résultat antérieur est
  signalé. Les tensions des nets sont aussi superposées au canevas lorsque
  l’option **Tensions des nets** est active.
- Historique nommé repliable : chaque entrée indique son état ; un clic ramène
  le schéma à cet état, dans le passé ou le futur.

Les nombres sont formatés en ingénierie et reparsés avant validation. Une valeur
invalide, un nom vide ou une transformation hors grille laisse document,
révision et compteur d'identifiants inchangés. Les messages utilisateur sont en
français ; les erreurs techniques restent en anglais. Un fil portant une erreur
ou un avertissement ERC est dessiné en rouge.

L'extraction est recalculée après chaque commande acceptée. Le panneau ERC
affiche les compteurs et les diagnostics triés ; un clic sélectionne l'objet
correspondant s'il existe. L'ERC est un rapport : seule une erreur structurelle
d'extraction bloque les nets et les analyses.

## 5. Instruments

Les contrôles acceptent `5m`, `10u`, etc. Le bouton de lancement est désactivé
sans extraction valide, pendant une analyse ou si le worker est indisponible.
Les résultats conservent la révision et les noms du document ; un résultat
antérieur est signalé mais reste consultable. **Marche/Arrêt** dans la barre
d’outils et `Espace` pilotent la session continue ; **Reset** et `F5` la
remettent à `t = 0`, **Pas** avance d’un pas en pause. Le recalcul après
modification est actif par défaut ; l’option **Auto** permet de le suspendre.
Les changements de paramètres restent appliqués à chaud sans remise à zéro.
L’échelon de source est désactivé par défaut pour ne pas fausser le démarrage
live.

- **Portées** : le panneau inférieur unique empile une ou plusieurs portées
  redimensionnables. Chaque portée accepte plusieurs tensions et courants, avec
  légende, valeurs courantes, zoom, déplacement, double-clic d’auto-échelle et
  infobulle. Le menu **Affichage** règle la superposition ou la séparation des
  signaux, l’échelle Y automatique ou manuelle et la vue temporelle ou FFT. Les
  curseurs A/B se posent au clic et affichent tA, tB, Δt et les valeurs par
  signal. **Figer** conserve le tracé, **Effacer** le vide, les flèches
  réordonnent, **Fusionner** joint la portée suivante et **Supprimer** la retire.
  Les signaux tracés sont rappelés par un marqueur de couleur sur le fil ou le
  composant correspondant. Le panneau reste accessible par **Afficher →
  Instruments et portées**. La fenêtre temporelle couvre **10 s de temps
  simulé** par défaut et se règle de 0,5 à 30 s dans les contrôles. Le bouton
  **Recentrer** réinitialise le zoom et le déplacement X/Y ; **Monter** et
  **Descendre** réordonnent les portées et sont désactivés aux extrémités.
- **Contrôles live** : état `En marche`, `Pause`, `Arrêtée` ou `Circuit
  invalide`, rapport temps simulé/réel, badge `trop lent (×0,3)`, vitesse de
  simulation logarithmique, vitesse des points de courant et luminosité de
  puissance. Le lien **Voir le diagnostic** ouvre l’inspecteur/ERC.
- **Fenêtre glissante** : la portée temps réel affiche les derniers
  échantillons sans plage d’arrêt. Les mesures min/max, crête-à-crête, moyenne,
  RMS et nombre d’échantillons restent disponibles lorsque le signal est
  sélectionné.
- **Sondes** : l’**Ohmmètre** reste disponible dans la palette et se pose sur un
  composant. Sa valeur apparaît dans la section **Sondes** du panneau avec
  **Afficher** et, quand un signal existe, **Graphique**. Les anciens composants
  **Voltmètre** et **Ampèremètre** restent lisibles dans les projets existants
  mais ne sont plus proposés. Le survol d’un fil affiche directement sa tension
  puis son intensité, référencées à la masse commune.
- **Spectre** : réutilise le dernier transitoire, choisit un signal et affiche
  pic non continu, fréquence et résolution. Il reste à la demande.
- **Bode** : analyse AC logarithmique, source, amplitude, sortie ; gain et phase
  partagent l'axe des fréquences. Un clic pose un marqueur de fréquence et
  affiche le gain et la phase au point choisi. Il reste à la demande.
- **Export CSV** : copie ou enregistre/télécharge toutes les valeurs de l'onglet
  actif ; le nom proposé dépend de l'onglet.

Un échec de validation, refus du worker ou échec de solveur est affiché en rouge
et ne remplace pas le dernier résultat valide. Pendant l'attente, les boutons sont
désactivés et le panneau se rafraîchit toutes les 20 ms. Les détails de calcul et
les formats de fichiers sont dans [INSTRUMENTS.md](INSTRUMENTS.md).

## 6. Raccourcis

| Touche | Action |
| --- | --- |
| `Ctrl+K` | palette de commandes |
| `W`, `J`, `N` | Fil, Jonction, Label |
| `R`, `M` | pivoter, miroir |
| `R`, `F` | orienter/miroiter l’aperçu de placement |
| `Maj` / `Ctrl` + clic | ajouter / inverser dans la sélection |
| `Maj` pendant un fil | changer le coude |
| `Alt` + clic | sélectionner le net complet |
| `Ctrl+C`, `Ctrl+V`, `Ctrl+D` | copier, coller, dupliquer |
| `Espace` | marche/arrêt de la simulation continue |
| `F5`, `Ctrl+Entrée` | reset de la session à `t = 0` |
| `Alt+↑`, `Alt+↓` | cycler la série E du paramètre |
| `Suppr`, `Retour arrière` | supprimer |
| `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Maj+Z` | annuler, rétablir |
| `Ctrl+A` | tout sélectionner |
| `?`, `F1` | aide et raccourcis |
| `Ctrl+Maj+A` | préférences d’affichage et accessibilité |
| `Home` | ajuster la vue au schéma |
| `Ctrl+Home` | cadrer la sélection |
| `+`, `-`, `0` | zoom avant, arrière, 100 % |
| `Ctrl+0` | origine et zoom par défaut |
| Flèches, `Maj`+flèches | déplacer la vue |
| `Ctrl+1`…`Ctrl+4` | Tout, Édition, Simulation, Canevas |
| `Échap` | annuler l'outil, le collage ou la sélection |

Les raccourcis sont ignorés si un champ texte a le focus, à l’exception de
`Ctrl+K` pour la palette de commandes. `Espace` et `F5` pilotent la session
continue sans ouvrir de fenêtre. La fenêtre **Aide** est générée depuis la même
table que les infobulles, afin d’éviter les divergences. La palette de commandes
reprend les touches des outils et propose **Projet** et **À propos**.

## 7. Atomicité, limites et tests

Toute création alloue l'identifiant sur un clone et n'exécute la commande
qu'après validation. Une action refusée ne change ni contenu, ni révision, ni
`next_id`. L'undo restaure les objets complets mais pas le compteur
d'identifiants. Collections et rendu sont ordonnés.

Limites : worker et session temps réel dans le fil de l'interface, tests
navigateur non automatisés, transitoire à pas fixe, FFT plafonnée, pas de
PNG/persistance de projet, opamp sans alimentation/pôle, pas de mobile
spécifique, fils non extensibles par une extrémité connectée. Les panneaux sont
masquables et redimensionnables, mais il n'y a pas de dockage libre ni de
disposition persistée. Le panneau unifié couvre le tracé temporel, la FFT, les
signaux multiples, les curseurs A/B, le gel, le zoom, la séparation, la fusion et
l’export CSV ; le mode XY, la fusion par glisser et le Bode incrusté ne sont pas
proposés. Les préférences et les projets JSON sont persistés ; le partage de
projet se fait par copie du JSON ou téléchargement.

Tests : `cargo test -p circuitsim-app` couvre document, vue, navigation,
mini-carte, édition, palette, propriétés, historique, presse-papiers, sélection
rectangle, séries E, ERC, canevas headless, dispositions, curseurs, marqueur
Bode, instruments, worker, FFT, Bode, CSV et export, ainsi que le déplacement de
fil sans embout, les fils en erreur, le basculement d'interrupteur et le
propriétés par double-clic, le mapping tension→couleur, les calques et les
palettes. Détails dans
[TESTING.md](TESTING.md) ; recette web dans [DELIVERY.md](DELIVERY.md).
