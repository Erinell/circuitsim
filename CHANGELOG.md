# Changelog

Toutes les modifications notables sont documentées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le versionnage
sémantique.

## [Unreleased]

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
