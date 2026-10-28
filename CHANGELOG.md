# Changelog

Toutes les modifications notables sont documentées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le versionnage
sémantique.

## [Unreleased]

### Added

- **Transistor MOSFET** à enrichissement, trois broches `D`/`G`/`S`, bulk relié à
  la source : loi carrée niveau 1 (triode, saturation, `lambda`), résistances
  d'accès `rd`/`rs`, diode de corps, capacités `cgs`/`cgd`/`cds` et permutation
  électrique `swap_ds`. Modèle DC, transitoire et AC (`gm`/`gds`) dans
  `sim-core`, symbole et inspecteur avec choix du canal N/P, extraction, valeurs
  live et animation des fils. Défauts ancrés sur les fiches 2N7002 et BSS84. Le
  symbole dessine la diode de corps et met en miroir vertical les liaisons
  drain/source, le bulk et cette diode quand `swap_ds` est actif, sans déplacer
  les points de connexion.
- Référence shunt programmable TL431/LM431 à trois broches (REF, K, A), avec
  paramètres éditables et modèle non linéaire DC, transitoire et petits signaux AC.
- Comparateur LM393 à collecteur ouvert avec broches d'alimentation, paramètres
  d'offset/sink éditables, pull-up externe et modèle DC, transitoire et AC par état.
- **Surintensités** sur le canevas : liseré ambre de 80 à 100 % et rouge au-delà
  de 100 %, badge `I / Imax` sur les fils, contour et triangle `!` sur les
  composants. La couleur de tension est conservée et aucun halo de puissance
  n’est réutilisé. Le survol donne courant, limite, pourcentage et sévérité.
- Paramètre `imax` étendu aux résistances, condensateurs, bobines, sources,
  interrupteurs et Zeners, avec une **limite de fil/net** réglable dans les
  **Préférences d’affichage** (10 A par défaut, comme les sources de tension).
- Portées : ligne de limite ambre, segments rouges au-delà et annotation de pic,
  de pourcentage et de durée.
- Liste **Surintensités** cliquable dans l’inspecteur pour sélectionner et cadrer
  le composant ou le fil concerné. Aucune alerte n’est produite sans simulation à
  jour.
- Choix **Diode / Diode Schottky** dans l’inspecteur, avec préréglages `vf`,
  `if`, `n`, `imax` et dérivation de `is`. Le symbole Schottky est dessiné avec
  ses crochets de cathode.
- Paramètres de la **LED** : longueur d’onde/couleur (`wavelength`), tension
  directe `vf`, courant nominal `if`, courant maximal `imax`. Le modèle dérive
  `is` de `vf`/`if` et le halo prend la couleur de la longueur d’onde.
- Paramètre de **résistance série** pour la bobine, utilisée en DC, transitoire
  et AC.
- Le circuit de démonstration **RC** est remplacé par le montage fourni :
  `V1 = 10 V`, `R1 = 1 kΩ`, `C1 = 100 µF`, `SW1` fermé, masse commune et
  sortie `Vc`.
- Panneau inférieur unique **Instruments et portées** : les contrôles live, les
  sondes, l’export et les portées empilées partagent désormais la même zone, avec
  les onglets **Portées**, **Spectre** et **Bode**. Les sondes sont regroupées
  avec les signaux directement dans les portées.
- Les portées peuvent afficher les tensions de **condensateurs** (`U(C1)`), les
  courants de **nets** (`I(nom)`) et les courants de **composants** (`I(R1)`,
  `I(V1)`, etc.). La tension d’un condensateur représente sa charge ; elle est
  aussi visible au survol et dans l’inspecteur.
- Un composant déposé au bout d’un fil ou sur une broche aligne maintenant une de
  ses broches sur la cible, au lieu de centrer le symbole sur le point d’accroche.
- Nouvel exemple **RLC parallèle commuté** dans **Fichier → Exemples**, avec
  `V1 = 5 V`, `R1 = 10 Ω`, `L1 = 1 H`, `C1 = 15 µF`, `R2 = 100 Ω` et `SW1`
  ouvert.
- L’exemple **Diviseur** adopte la disposition demandée : source à droite,
  masse à gauche, `OUT` entre les deux résistances de 1 kΩ ; la sortie reste à
  2,5 V.
- L’exemple **RLC** adopte la disposition demandée : masse en bas à gauche,
  `R1`, `L1`, `C1` et `OUT` à droite ; la sortie continue vaut 5 V.

### Changed

- **Simulation temps réel** : budget de 5 ms de calcul par image au lieu de
  800 pas fixes, pas live réglable de 1 µs à 10 ms (1 ms par défaut), un
  échantillon par pas d'intégration et tampon en anneau de 10 000 échantillons
  sans `Vec::drain` par image. Le pas effectif, la fenêtre conservée et le badge
  de ralentissement restent visibles ; les analyses complètes gardent le pas
  saisi, tous leurs échantillons, les CSV et les spectres.
- L'en-tête place désormais menus et commandes sur une même rangée, avec les
  actions centrées ; les fenêtres étroites conservent un retour à la ligne centré.
  L'alignement vertical est harmonisé, la recherche est visible et l'état de
  simulation plus lisible.
- La liste de la palette de commandes occupe toute sa largeur ; sa barre de
  défilement reste à droite de la fenêtre.
- Les textes secondaires passent à 11 px et les boutons bénéficient d'un peu plus
  d'espace pour améliorer leur lisibilité et leur confort de clic. Leur couleur
  est adaptée au thème et vérifiée à 4,5:1 dans les trois thèmes.
- Le menu **Afficher** est renommé **Affichage**. La limite des fils/nets est
  déplacée dans la fenêtre **Préférences d’affichage**.
- Les raccourcis à une lettre pour placer les composants sont retirés ; le
  placement passe uniquement par la palette et le menu **Dessin**. `R` pivote
  de nouveau la sélection et `M` la met en miroir.
- Le recalcul après modification est actif par défaut ; l’option **Auto** permet
  encore de le suspendre.
- Les portées affichent une fenêtre glissante de **10 s de temps simulé** par
  défaut, indépendante de la vitesse de lecture. La durée est réglable de 0,5 à
  10 s dans les contrôles. Un seul anneau de 10 000 échantillons alimente les
  portées et le spectre, sans recopie périodique.
- Les boutons **Monter** et **Descendre** réordonnent la portée par son
  identifiant ; ils sont désactivés aux extrémités. **Recentrer** réinitialise le
  zoom et le déplacement X/Y de la portée active.
- Les composants **Voltmètre** et **Ampèremètre** sont retirés de la palette et
  des menus de placement ; l’**Ohmmètre** reste disponible. Les anciens symboles
  restent lisibles dans les projets existants.
- Le survol d’un fil ou d’une broche affiche l’intensité sous la tension, avec la
  masse commune comme référence.
- Dans les portées, **Ajouter une portée** passe dans les contrôles et les
  valeurs/mesures s’affichent à droite du tracé. La section **Sondes** est repliée
  par défaut, ce qui rend de la hauteur au graphique.
- Plusieurs symboles de masse sur un même canevas sont fusionnés dans le nœud
  commun `0`, même sans fil partagé. L’ERC signale la fusion en information au
  lieu de la refuser.
- Les valeurs et références sont placées au plus près du corps du symbole, avec
  un décalage adapté à son orientation.
- La zone de clic, le cadre de sélection et la marquise épousent le corps des
  composants au lieu d’ajouter une cellule de marge.
- Le halo de puissance est réservé aux composants non résistifs ; celui des LED
  est plus petit, plus discret et peint derrière le symbole.

### Fixed

- Un clic sur un diagnostic ERC cadre maintenant l'objet sélectionné dans le
  canevas, comme pour les alertes de surintensité.

- Un interrupteur **ouvert** ne fait plus apparaître de fuite dans les
  graphiques : `roff` passe à `1e18 Ω` par défaut, son courant est rapporté
  exactement à zéro, et les traces sous `1 pV` / `1 fA` sont ramenées à zéro.
- Au démarrage et après **Reset**, les condensateurs du transitoire partent à
  **tension nulle** (charge complètement déchargée) au lieu du point DC à
  condensateur ouvert. Le calcul DC classique reste inchangé.
- Le panneau des portées n’affiche plus de barre de défilement superflue ni de
  séparateur au-dessus de la première portée. Ajouter une portée conserve des
  hauteurs identiques pour tous les tracés.
- Le courant animé s’arrête quand l’interrupteur est ouvert, y compris lorsque le
  solveur renvoie un courant de fuite résiduel.
- Les sources de tension des exemples diviseur, LED, interrupteur et RLC sont
  orientées dans le bon sens ; les signes `+` et `-` suivent les bornes.
- La fenêtre native s’ouvre réellement maximisée sous Windows.
- Le menu contextuel conserve la cible et le contenu du clic droit même si la
  souris se déplace ensuite.

## [0.5.0] - 2026-10-27

### Added

- Simulation continue au démarrage avec Marche/Arrêt, Reset, Pas et raccourcis
  `Espace`/`F5`.
- Mise à jour à chaud des paramètres et interrupteurs sans remise à zéro quand
  la topologie reste compatible.
- Contrôles live de vitesse, d’animation de courant et de luminosité de
  puissance, avec rapport temps simulé/réel et badge de retard.
- Fils colorés selon la tension, légende et palettes daltoniennes.
- Calques indépendants pour tension, courant, puissance, valeurs, références et
  labels, avec taille des valeurs et placement évitant les fils.
- Thème clair, petite grille, halos de puissance et styles de résistance
  européen/américain.
- Lecture au survol, câblage par glisser depuis une broche, menus contextuels
  enrichis, coupe de fil et mesures directes.
- Raccourcis de placement à une lettre et barre d’outils compacte pilotée par
  les favoris.
- Portées incrustées empilables dans le canevas, avec signaux multiples, gel,
  zoom et ajout par clic droit.
- Curseurs de réglage sur le canevas et galerie d’exemples originaux (RC,
  diviseur, LED, interrupteur, RLC).
- Portées incrustées consolidées : échelle automatique ou manuelle, signaux
  superposés ou séparés, FFT, curseurs A/B, réordonnancement, fusion, cache de
  traces et marqueurs de couleur sur le schéma.
- Préférences versionnées persistées en natif et dans `localStorage`, avec
  restauration des thèmes, calques, favoris, vitesses et panneaux visibles.
- Écran **À propos** avec version, licence et licences tierces.
- Projets JSON versionnés : copie, chargement, lecture/enregistrement natif,
  téléchargement web et revalidation avant remplacement du document.
- Aide générée depuis la table unique des raccourcis, panneaux repliables et
  animations limitées à 150 ms.
- Menus **Fichier**, **Édition**, **Dessin**, **Afficher**, **Options** et
  **Aide**, et déplacement de la vue au glisser en lecture seule.
- Palette de commandes enrichie (**Projet**, **À propos**) et contrastes des
  trois thèmes vérifiés par test.
- Barre supérieure allégée : les boutons dupliqués par les menus sont retirés ;
  seuls les contrôles de simulation restent visibles. Les outils, composants et
  exemples sont dans les menus et la palette.

### Changed

- L’échelon de source est désactivé par défaut pour le mode live.
- La barre d’outils remplace **Simuler** par les contrôles de session continue.

### Fixed

- La hauteur des portées incrustées suit désormais la hauteur de la section et
  se répartit entre les portées visibles.

## [0.4.0] - 2026-10-26

### Added

- Fenêtre de propriétés d’un composant ouverte par double-clic sur le canevas.
- Basculement direct d’un interrupteur par un clic et halo d’allumage des LED.
- Tracé rouge des fils portant une erreur ou un avertissement ERC.

### Changed

- Le déplacement d’un segment coupe ses extrémités au lieu de créer des embouts.
- Le flux de courant suit le sens réel des injections et contraste avec les fils.
- L’inspecteur est allégé : aides de saisie dans les infobulles et sections compactées.

### Fixed

- Le sens de l’animation du courant est déterminé par parcours du net depuis
  l’injection dominante, avec priorité aux courants positifs.

## [0.3.0] - 2026-10-26

### Added

- Fenêtre native maximisée et fenêtres d’aide, de démarrage et d’accessibilité centrées.
- Déplacement de segment préservant les connexions par rajout de courts embouts.
- Redimensionnement des extrémités libres de fils.
- Fils plus épais et animation du courant issue du dernier transitoire.
- Sondes voltmètre, ampèremètre et ohmmètre plaçables, listées avec affichage et tracé optionnels.
- Transitoire temps réel sans plage d’arrêt, avec vitesse, pause et fenêtre glissante.

### Changed

- Les valeurs de tension ne sont plus superposées aux labels du canevas.
- Le panneau Instruments remplace les champs Début/Arrêt par une session temps réel.

## [0.2.0] - 2026-10-26

### Added

- Gouvernance Git, workflow de branches et conventions de commits.
- Licence `MIT OR Apache-2.0`, vérification `cargo-deny` et licences tierces.
- Palette de commandes `Ctrl+K` avec recherche floue et actions clavier.
- Palette de composants avec catégories, favoris et récents.
- Aperçu de placement orientable, placement répété et insertion en série.
- Accroche magnétique, routage orthogonal évitant les symboles et câblage fluide.
- Déplacement de segments avec reroutage des voisins.
- Surbrillance et sélection complète des nets, autocomplétion des labels.
- Alignement et distribution de la sélection avec guides visuels.
- Sélection rectangle avec modes inclusion/toucher, ajout `Maj` et bascule `Ctrl`.
- Presse-papiers interne, collage avec aperçu fantôme, duplication `Ctrl+D` et
  rotation/miroir de groupe.
- Historique nommé avec sauts d’état et panneau dédié.
- Saisie directe `4k7`/`100n` et cycle E12/E24/E96 à la molette, au clavier ou
  par boutons dans l’inspecteur.
- Inspecteur contextuel des tensions, courants et puissances du dernier
  transitoire, avec signalement d’un résultat antérieur.
- Menus contextuels spécifiques aux symboles, labels, fils, jonctions et au vide.
- Aide `?` et infobulles des actions principales.
- Zoom et déplacement clavier, cadrage de la sélection et coordonnées du curseur.
- Mini-carte navigable montrant contenu, sélection et fenêtre visible.
- Menu **Vue**, panneaux masquables et dispositions Tout, Édition, Simulation et
  Canevas, sans dockage libre.
- Bouton **Simuler** et raccourci `Ctrl+Entrée`/`F5` pour relancer le transitoire.
- Rafraîchissement automatique optionnel après modification, avec délai.
- Tensions des nets superposées au canevas et badge d’état du dernier
  échantillon, avec signalement d’un résultat antérieur.
- Deux curseurs temporels A/B sur la portée, avec Δt, 1/Δt, valeurs, min, max et
  RMS sur l’intervalle.
- Marqueur de fréquence sur le Bode, avec gain et phase au point choisi.
- Guide de démarrage rapide, accès à l’exemple RC et état vide plus explicite.
- Préférences d’affichage : thème sombre ou contraste élevé, taille de
  l’interface et réduction des animations.

### Changed

- Documentation réorganisée et allégée.

## [0.1.0] - 2026-10-26

### Added

- Import initial de CircuitSim : cœur analogique, éditeur, protocole worker,
  interface egui, instruments, tests, exemples, benchmarks, CI et distribution
  web.
- Licence `MIT OR Apache-2.0`.
