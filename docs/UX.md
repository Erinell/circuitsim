# Ergonomie et interactions

## 1. Principes

- Manipulation directe : le canevas montre le résultat avant validation.
- Aucun mode caché : l’outil actif, l’aperçu et le raccourci sont visibles.
- Une action acceptée est annulable en une commande ; une action refusée ne
  modifie rien.
- Le clavier complète la souris ; les commandes fréquentes sont accessibles sans
  quitter le canevas.
- Le retour visuel est immédiat : survol, cible d’accroche, net complet et guides
  d’alignement.

## 2. Raccourcis

| Touche | Action |
| --- | --- |
| `Ctrl+K` | Palette de commandes |
| `V`, `W`, `J`, `L` | Sélection, Fil, Jonction, Label |
| `R`, `M` | Pivoter ou mettre en miroir la sélection |
| `R`, `F` | Pivoter ou mettre en miroir l’aperçu de placement |
| `Maj` + clic | Ajouter ou retirer de la sélection |
| `Alt` + clic | Sélectionner le net complet |
| `Maj` pendant un fil | Changer le coude horizontal/vertical |
| `Ctrl+A` | Tout sélectionner |
| `Suppr` / `Retour arrière` | Supprimer |
| `Ctrl+Z`, `Ctrl+Y` | Annuler, rétablir |
| `Home` | Ajuster la vue |
| `Échap` | Annuler l’outil, le fil ou la sélection |

Les raccourcis sont ignorés lorsqu’un champ texte a le focus.

## 3. Placement et palettes

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

## 4. Câblage

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

## 5. Alignement et édition

- Le menu **Aligner** aligne gauche, centre, droite, haut, milieu ou bas.
- La distribution horizontale ou verticale conserve les objets extrêmes et
  espace les centres intermédiaires.
- Pendant un déplacement, des guides verticaux et horizontaux apparaissent
  lorsque des symboles partagent une coordonnée avec un symbole fixe.
- Le dialogue de label propose les noms de nets existants en autocomplétion.

## 6. Résultats et erreurs

Une opération refusée affiche un message français dans la barre d’état et ne
modifie ni le schéma, ni la révision, ni `next_id`. Les erreurs techniques
restent typées dans les crates.

## 7. Critères d’acceptation

- Construire un RC, le câbler et lancer la simulation en six actions maximum.
- Aucune interaction de survol, glisser ou câblage ne dépasse 16 ms sur le
  schéma de démonstration.
- Toute modification de placement, insertion, déplacement ou alignement reste
  annulable en une action.
- Les tests natifs et proptest vérifient grille, connectivité, déterminisme et
  restauration après undo.

## 8. Périmètre de cette version

La palette de commandes, les favoris/récents, l’insertion en série, l’accroche,
le routage, les segments, la surbrillance de net, les guides et l’alignement sont
livrés. Les sélections rectangle, édition directe, panneaux dockables, thèmes,
simulation interactive, scope avancé et onboarding restent dans les lots G3 à
G7. Voir [UI.md](UI.md) et [EDITOR.md](EDITOR.md).
