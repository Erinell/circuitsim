# Ergonomie et interactions

## 1. Principes

- Manipulation directe : le canevas montre le résultat avant validation.
- Aucun mode caché : l’outil actif, l’aperçu et le raccourci sont visibles.
- Une action acceptée est annulable en une commande ; une action refusée ne
  modifie rien.
- Le clavier complète la souris ; les commandes fréquentes sont accessibles sans
  quitter le canevas.
- Le retour visuel est immédiat : survol, cible d’accroche, net complet, guides,
  rectangle de sélection et aperçu de collage.

## 2. Raccourcis

| Touche | Action |
| --- | --- |
| `Ctrl+K` | Palette de commandes |
| `V`, `W`, `J`, `L` | Sélection, Fil, Jonction, Label |
| `R`, `M` | Pivoter ou mettre en miroir la sélection |
| `R`, `F` | Pivoter ou mettre en miroir l’aperçu de placement |
| `Maj` + clic | Ajouter ou retirer de la sélection |
| `Ctrl` + clic | Inverser un objet dans la sélection |
| `Alt` + clic | Sélectionner le net complet |
| `Maj` pendant un fil | Changer le coude horizontal/vertical |
| `Ctrl+A` | Tout sélectionner |
| `Ctrl+C`, `Ctrl+V` | Copier, coller avec aperçu |
| `Ctrl+D` | Dupliquer la sélection |
| `Suppr` / `Retour arrière` | Supprimer |
| `Ctrl+Z`, `Ctrl+Y` | Annuler, rétablir |
| `Alt+↑`, `Alt+↓` | Valeur E série suivante ou précédente |
| `+`, `-`, `0` | Zoom avant, arrière, 100 % |
| `Ctrl+0` | Origine et zoom par défaut |
| Flèches, `Maj`+flèches | Déplacer la vue |
| `Home`, `Ctrl+Home` | Cadrer tout le schéma, cadrer la sélection |
| `Ctrl+1`…`Ctrl+4` | Tout, Édition, Simulation, Canevas |
| `?` | Ouvrir ou fermer l’aide |
| `Échap` | Annuler l’outil, le fil, le collage ou la sélection |

Les raccourcis sont ignorés lorsqu’un champ texte a le focus, à l’exception de
`Ctrl+K` pour ouvrir ou fermer la palette de commandes.

## 3. Sélection

- Un clic sélectionne l’objet sous le curseur. Un clic sur le vide efface la
  sélection.
- Un rectangle tracé de gauche à droite sélectionne les objets entièrement
  inclus ; un rectangle tracé de droite à gauche sélectionne les objets touchés.
- `Maj` ajoute le résultat du rectangle, `Ctrl` inverse chaque objet touché.
- `Ctrl+A` sélectionne tous les objets éditables.
- `Alt` + clic sélectionne le net complet à partir d’une broche, d’un fil, d’une
  jonction ou d’un label.
- Un glisser sur un symbole, label ou jonction déplace toute la sélection ; un
  glisser sur un fil déplace le segment perpendiculairement.

## 4. Presse-papiers et duplication

- `Ctrl+C` copie les objets sélectionnés dans un presse-papiers interne. Les
  identifiants, l’orientation et les paramètres sont conservés, les références
  sont régénérées au collage.
- `Ctrl+V` arme un aperçu fantôme : un clic colle le contenu autour du curseur,
  plusieurs clics répètent le collage, `Échap` annule.
- `Ctrl+D` copie et colle immédiatement avec un décalage de deux cases, puis
  sélectionne les nouveaux objets.
- Le menu contextuel propose Copier, Coller ici, Dupliquer et Supprimer selon
  l’objet sous le curseur.
- Une rotation ou un miroir de groupe transforme les positions autour du centre
  de la sélection et reste une seule action annulable.

## 5. Navigation et disposition

- La molette zoome autour du curseur ; `+` et `-` autour du centre du canevas,
  `0` revient à 100 %, `Ctrl+0` centre l’origine au zoom par défaut.
- Les flèches déplacent la vue de deux cases, `Maj` de huit cases. Le bouton
  milieu reste disponible pour un déplacement libre.
- `Home` cadre tout le contenu ; `Ctrl+Home` cadre la sélection.
- La mini-carte montre le contenu, la sélection et la fenêtre visible. Un clic
  recentre la vue sur le point correspondant.
- Le menu **Vue** masque la palette, l’inspecteur, les instruments ou la
  mini-carte. Quatre dispositions fiables sont proposées : Tout, Édition,
  Simulation et Canevas. Elles restent des zones fixes redimensionnables ; le
  dockage libre n’est pas proposé tant qu’il ne serait pas fiable.
- La barre d’état affiche le curseur en coordonnées de grille, le nombre de nets
  et de composants, la sélection et le zoom courant.

## 6. Placement et palettes

- La palette de commandes filtre les composants et actions par sous-séquence,
  sans tenir compte des accents. `Entrée` place le composant au dernier curseur
  de canevas ; sans curseur, il arme l’outil de placement.
- La palette de composants propose les catégories Passifs, Sources et
  connexions, Semi-conducteurs et Personnalisés. Le clic droit ajoute ou retire
  un favori ; les cinq derniers composants utilisés restent visibles.
- Le placement est répétitif : le composant reste armé après un clic jusqu’à
  `Échap`.
- L’aperçu fantôme suit la grille et affiche les broches. `R` tourne l’aperçu,
  `F` le met en miroir.
- Un composant à deux broches déposé sur un fil est inséré en série : le fil est
  supprimé, le composant ajouté et les deux tronçons créés dans une seule
  commande annulable.

## 7. Câblage

- Le départ et l’arrivée peuvent être une grille libre, une broche ou un point
  de fil. Un rayon de deux cases aimante le point le plus proche ; un anneau
  cyan montre la cible.
- Le tracé est orthogonal et évite les corps de symboles. `Maj` inverse le
  premier coude (horizontal ou vertical).
- `Échap` annule le fil courant. Un second clic sur la même position l’annule
  aussi.
- Un fil sélectionné peut être glissé perpendiculairement. Les fils
  perpendiculaires partageant une extrémité suivent le déplacement ; une
  topologie ambiguë (T, jonction ou label sur le segment) est refusée sans
  modification.
- Un clic sur une broche ou un fil met en évidence le net complet. `Alt` + clic
  sélectionne tous ses objets.

## 8. Inspecteur, valeurs et historique

- L’inspecteur affiche la référence, l’orientation, le miroir et les paramètres.
- Les valeurs s’écrivent directement en notation d’ingénieur : `4k7`, `100n`,
  `2.2u`, `1e-6`, avec unité optionnelle. `Entrée` applique la saisie.
- La série préférée E12, E24 ou E96 se choisit dans l’inspecteur. La molette,
  `Alt+↑`/`Alt+↓` ou les boutons `−`/`+` parcourent les valeurs normalisées en
  conservant l’ordre de grandeur.
- Une valeur invalide affiche immédiatement un message local ; l’application
  reste refusée tant que la saisie n’est pas corrigée.
- Pour un symbole, la section **Grandeurs simulées** affiche le dernier
  échantillon transitoire : tension par broche et par net, courant de branche
  ou courant dérivé d’une résistance, puis puissance. Un résultat antérieur aux
  dernières modifications est signalé.
- L’historique nommé liste les modifications appliquées et rétablies. Cliquer
  un état ramène le schéma exactement à cet état, sans perdre les commandes
  futures.
- Le menu contextuel est spécifique à la cible : symbole (pivoter, miroir),
  label (renommer, sélectionner le net), fil ou jonction (sélectionner le net),
  puis copier, dupliquer, supprimer. Sur le vide, il propose coller, tout
  sélectionner, ajuster, annuler et rétablir.

## 9. Résultats et erreurs

Une opération refusée affiche un message français dans la barre d’état et ne
modifie ni le schéma, ni la révision, ni `next_id`. Les erreurs techniques
restent typées dans les crates.

## 10. Critères d’acceptation

- Construire un RC, le câbler et lancer la simulation en six actions maximum.
- Aucune interaction de survol, glisser, rectangle ou câblage ne dépasse 16 ms
  sur le schéma de démonstration.
- Toute modification de placement, insertion, collage, déplacement, alignement
  ou transformation reste annulable en une action.
- Les tests natifs et proptest vérifient grille, connectivité, déterminisme,
  séries E, presse-papiers, navigation et restauration après undo.
- Les quatre dispositions gardent un canevas utilisable, sans panneau
  superposé ; la mini-carte reste cohérente après zoom et déplacement.

## 11. Périmètre de cette version

La sélection rectangle, le presse-papiers, la duplication, la rotation et le
miroir de groupe, l’édition directe E12/E24/E96, l’inspecteur avec valeurs
simulées, les menus contextuels, l’historique nommé, les dispositions fiables et
la mini-carte sont livrés. Le dockage libre, les thèmes, la simulation
interactive, le scope avancé et l’onboarding restent dans les lots G5 à G7. Voir
[UI.md](UI.md) et [EDITOR.md](EDITOR.md).
