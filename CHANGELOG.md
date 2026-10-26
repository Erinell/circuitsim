# Changelog

Toutes les modifications notables sont documentées ici. Le format suit
[Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le versionnage
sémantique.

## [Unreleased]

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

### Changed

- Documentation réorganisée et allégée.

## [0.1.0] - 2026-10-26

### Added

- Import initial de CircuitSim : cœur analogique, éditeur, protocole worker,
  interface egui, instruments, tests, exemples, benchmarks, CI et distribution
  web.
- Licence `MIT OR Apache-2.0`.
