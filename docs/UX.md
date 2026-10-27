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
- La lisibilité est réglable sans changer le schéma : tension, courant,
  puissance, valeurs, références et labels sont des calques indépendants.

## 2. Raccourcis

| Touche | Action |
| --- | --- |
| `Ctrl+K` | Palette de commandes |
| `W`, `J`, `N` | Fil, Jonction, Label |
| `R`, `C`, `L`, `D`, `V`, `G`, `S` | Placer résistance, condensateur, bobine, diode, source, masse, interrupteur |
| `Maj+R`, `M` | Pivoter ou mettre en miroir la sélection |
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
| `Espace` | Marche/arrêt de la simulation continue |
| `F5`, `Ctrl+Entrée` | Reset de la session à `t = 0` |
| `?` / `F1` | Ouvrir ou fermer l’aide |
| `Ctrl+Maj+A` | Préférences d’affichage et accessibilité |
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
- Un fil sélectionné peut être glissé perpendiculairement. Le segment est
  simplement remplacé à sa nouvelle position : ses anciennes extrémités sont
  libérées, aucun embout ni fil supplémentaire n'est créé. Les objets qui
  touchaient le segment restent à leur place et peuvent devenir non connectés.
- Une extrémité libre peut être tirée le long de l’axe pour allonger ou
  raccourcir le fil. Une extrémité connectée est refusée avec un message.
- Les fils sont plus épais ; après simulation, des points jaunes à halo sombre
  suivent le sens réel du courant estimé, avec une densité et une vitesse
  proportionnelles au courant. `Vue → Courant animé` masque ce flux.
- La couleur des fils suit la tension du net : bleu/gris/orange par défaut,
  variante cyan/gris/magenta, avec une légende. Le menu **Afficher** règle les
  calques Tension, Courant, Puissance, Valeurs, Références et Labels.
- La puissance dissipée ajoute un halo dont la luminosité est réglable. Le sens
  du courant peut être conventionnel ou électronique.
- Un fil signalé par l’ERC (erreur ou avertissement) est tracé en rouge, prioritaire
  sur la couleur de tension et la sélection.
- Une LED montre son allumage par un halo proportionnel à sa tension directe.
  Un clic sur un interrupteur le ferme ou l’ouvre directement sur le canevas ;
  l’action est annulable.
- Un double-clic sur un symbole ouvre une fenêtre centrée d’édition de ses
  propriétés, avec les mêmes validations que l’inspecteur.
- Le survol affiche une bulle avec le type, le nom, les paramètres, les tensions
  de broches, le courant et la puissance. Aucun clic n’est nécessaire.
- En mode Sélection, un glisser démarré sur une broche câble directement avec
  aperçu, accroche et annulation en une action. L’outil Fil `W` reste disponible.
- Le menu contextuel propose les propriétés, les transformations, la
  duplication, la suppression, les mesures et, sur un fil, la coupe, une
  jonction ou un label. Sur le fond, il propose les sous-menus **Ajouter**.
- La barre d’outils compacte affiche les composants favoris puis les composants
  fréquents, avec la lettre de placement en infobulle. La barre d’état rappelle
  l’outil actif.
- Un clic droit sur un composant propose **Ajouter un curseur**. Le curseur se
  règle au glisser ou à la molette, affiche la valeur SI et applique la
  modification à chaud en une seule étape annulable.
- Le menu **Exemples** propose des circuits originaux : RC, diviseur, LED,
  interrupteur et RLC.
- Un clic sur une broche ou un fil met en évidence le net complet. `Alt` + clic
  sélectionne tous ses objets.

## 8. Simulation interactive

- À l’ouverture d’un circuit valide, la session temps réel démarre seule. Le
  circuit est lisible et animé sans clic préalable.
- **Marche/Arrêt** dans la barre d’outils et `Espace` figent ou relancent la
  session. **Reset** et `F5` reviennent à `t = 0`. **Pas** avance d’un pas en
  pause.
- Une modification de paramètre (valeur, interrupteur) est appliquée à chaud :
  le temps simulé et les états des éléments sont conservés. Une modification de
  topologie relance la session en conservant la pause et la vitesse.
- Le pas de calcul reste réglable. La vitesse multiplie le temps simulé par
  seconde réelle ; un rapport temps simulé/réel et un badge `trop lent` rendent
  le retard visible.
- L’échelon de source est désactivé par défaut. Il reste disponible pour les
  scénarios qui en ont besoin.
- Les contrôles live règlent la vitesse de simulation (échelle logarithmique),
  la vitesse des points de courant et la luminosité de puissance.
- **Portées incrustées** : la zone en bas du canevas empile une ou plusieurs
  portées redimensionnables. Chaque portée accepte plusieurs tensions et
  courants, avec zoom, déplacement, infobulle, gel, effacement et suppression.
  Un clic droit sur un composant ou un fil ajoute le signal à la dernière
  portée. Le panneau classique reste accessible par **Vue**.
- La portée classique affiche une fenêtre glissante des derniers échantillons. Le tampon
  peut être mis en pause sans perdre les données ; il est plafonné en mémoire.
- Un circuit invalide affiche l’état `Circuit invalide` et un lien vers le
  diagnostic, sans modale bloquante.
- Les tensions ne sont plus superposées aux labels du canevas. Un badge indique
  `t = … · à jour/antérieur`, et l’inspecteur reprend les tensions par broche,
  le courant et la puissance.
- Les sondes **Voltmètre**, **Ampèremètre** et **Ohmmètre** se placent depuis la
  palette. Leur liste apparaît à côté du graphique avec **Afficher** et
  **Graphique**. Le voltmètre se pose sur un fil ; l’ampèremètre et l’ohmmètre
  mesurent le composant sous la sonde.
- Le calcul reste dans le fil de l’interface ; le worker dédié et
  `SharedArrayBuffer` restent hors périmètre pour ce lot.

## 9. Curseurs et lecture

- La portée place le premier clic sur A, le deuxième sur B, puis déplace le
  curseur actif. Les boutons **A**/**B** choisissent la cible ; un clic droit ou
  `Maj` force B.
- Les curseurs s’alignent sur l’échantillon temporel le plus proche. Une bande
  colorée marque l’intervalle et les lignes A/B restent visibles sur la courbe.
- Le panneau affiche tA, tB, Δt, 1/Δt, puis pour chaque signal les valeurs en A
  et B, leur écart, et min/max/RMS sur l’intervalle.
- **Effacer** retire les deux curseurs. Une nouvelle simulation conserve les
  temps et les recolle à l’échantillon le plus proche.
- Sur le Bode, un clic pose un marqueur de fréquence commun au gain et à la
  phase ; le panneau affiche fréquence, gain en dB et phase en degrés.

## 10. Accueil et accessibilité

- Le guide de démarrage présente quatre gestes : placer, câbler, simuler,
  explorer. Il propose l’exemple RC, un schéma vide et l’aide complète.
- Il s’ouvre au lancement et reste accessible par le bouton **Démarrage** ou la
  palette de commandes.
- L’état vide du canevas rappelle les trois actions essentielles et renvoie au
  guide.
- Les préférences d’affichage proposent les thèmes sombre, contrasté ou clair,
  une taille d’interface de 85 à 130 %, la réduction des animations, la petite
  grille et le style de résistance européen ou américain.
- `Tab` et `Maj+Tab` parcourent les contrôles, `Entrée` active, `Échap` ferme.
  Toutes les actions principales ont un libellé texte et une infobulle.
- Les préférences sont appliquées immédiatement et restent valables pour la
  session ; leur persistance est prévue ultérieurement.

## 11. Inspecteur, valeurs et historique

- L’inspecteur affiche la référence, l’orientation, le miroir et les paramètres.
  Un double-clic sur un symbole ouvre la même édition dans une fenêtre centrée.
- Les aides de saisie sont dans les infobulles des champs ; le panneau privilégie
  les valeurs, les sections et les actions. Les valeurs et références du schéma
  peuvent être affichées ou masquées, avec une taille de valeur réglable.
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
- Le menu contextuel est spécifique à la cible : symbole (propriétés, pivoter,
  miroir, mesurer), label (renommer, sélectionner le net), fil (couper, jonction,
  label, sélectionner le net) ou jonction, puis copier, dupliquer, supprimer.
  Sur le vide, il propose **Ajouter** par catégorie, coller, tout sélectionner,
  ajuster, annuler et rétablir.

## 12. Résultats et erreurs

Une opération refusée affiche un message français dans la barre d’état et ne
modifie ni le schéma, ni la révision, ni `next_id`. Les erreurs techniques
restent typées dans les crates. Les fils concernés par un diagnostic ERC non
informatif sont dessinés en rouge sur le canevas.

## 13. Critères d’acceptation

- Construire un RC, le câbler et lancer la simulation en six actions maximum.
- Aucune interaction de survol, glisser, rectangle ou câblage ne dépasse 16 ms
  sur le schéma de démonstration.
- Toute modification de placement, insertion, collage, déplacement, alignement
  ou transformation reste annulable en une action.
- Les tests natifs et proptest vérifient grille, connectivité, déterminisme,
  séries E, presse-papiers, navigation et restauration après undo.
- Les quatre dispositions gardent un canevas utilisable, sans panneau
  superposé ; la mini-carte reste cohérente après zoom et déplacement.
- Après une simulation, les tensions restent lisibles et le résultat antérieur
  est identifiable sans ambiguïté.
- Deux curseurs donnent un Δt et des mesures d’intervalle reproductibles, sans
  modifier le résultat ni le schéma.
- Un nouvel utilisateur trouve le guide, charge l’exemple et lance une
  simulation sans documentation externe.
- Le contraste élevé, la taille de l’interface et la réduction des animations
  s’appliquent immédiatement et restent utilisables au clavier.

## 14. Périmètre de cette version

La sélection rectangle, le presse-papiers, la duplication, la rotation et le
miroir de groupe, l’édition directe E12/E24/E96, l’inspecteur avec valeurs
simulées, les menus contextuels, l’historique nommé, les dispositions fiables,
la mini-carte, le calcul rapide, le rafraîchissement automatique, les curseurs de
portée, le marqueur Bode, le guide de démarrage et les préférences
d’accessibilité sont livrés. Le dockage libre reste hors périmètre et la
persistance des préférences appartient à la finalisation. Voir [UI.md](UI.md),
[INSTRUMENTS.md](INSTRUMENTS.md) et [EDITOR.md](EDITOR.md).
