# Changelog

Toutes les modifications notables sont documentées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le versionnage
sémantique.

## [Unreleased]

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

### Changed

- L’échelon de source est désactivé par défaut pour le mode live.
- La barre d’outils remplace **Simuler** par les contrôles de session continue.

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
