# Éditeur de schéma

`editor-core` contient le schéma, les commandes réversibles, la connectivité,
l'ERC et la sérialisation. Il ne dépend ni d'egui, ni du navigateur, ni d'une
horloge. Les panneaux sont décrits dans [UI.md](UI.md), les composants dans
[COMPONENTS.md](COMPONENTS.md).

## 1. Grille et transformations

- `GridPos { x, y }` utilise des `i32` bornés à `-1_000_000..=1_000_000`.
- L'axe `y` augmente vers le bas ; aucune coordonnée fractionnaire.
- Les déplacements utilisent des additions/soustractions vérifiées ; une
  commande nulle, vide ou avec doublon est refusée.
- Les broches sont transformées par miroir local (`x -> -x`), puis rotation
  `R0/R90/R180/R270`, puis translation par `Symbol.at`.
- `R90` applique `(x, y) -> (-y, x)`. Une broche hors grille produit
  `ExtractError::PinOutOfBounds`.

## 2. Symboles

Le registre intégré contient les onze types de [NETLIST.md](NETLIST.md). Chaque
`SymbolDef` valide un nom de type non vide (≤ 128 caractères), 1 à `u16::MAX`
broches, des noms de broches uniques et valides, des positions dans la grille
locale, des noms de paramètres valides et des valeurs numériques finies.

Un `SymbolKind::Custom(String)` est refusé s'il est déjà enregistré. Le MOSFET
n'est pas intégré : `examples/custom_mosfet.rs` illustre ses broches, l'extraction
et le JSON ; aucun modèle électrique n'est fourni. Un type absent du registre
produit `UnknownSymbolKind` à l'extraction.

## 3. Fils et topologie

Un `Wire` est un segment orthogonal fermé : longueur strictement positive,
`a.x == b.x` ou `a.y == b.y`, extrémités dans la grille et réordonnées
lexicographiquement. Diagonales et longueurs nulles sont refusées.

| Situation | Connexion | Diagnostic |
| --- | --- | --- |
| Extrémités au même point | oui | aucun |
| Extrémité sur l'intérieur d'un autre fil (T) | oui | aucun |
| Croisement perpendiculaire sans point | non | aucun |
| Croisement perpendiculaire avec `Junction` | oui | aucun |
| Chevauchement colinéaire | oui | `WireOverlap` |
| Jonction hors fil | non applicable | `JunctionOffWire` |
| Jonction sur un seul fil ou une broche | non applicable | `RedundantJunction` |

Une jonction relie tous les fils qui contiennent sa position. Un label ne
remplace pas un composant : un fil seulement décoré par un label garde une
extrémité non connectée.

## 4. Labels et extraction

Un `NetLabel` possède une position et un texte validé, coupé des espaces. Il
s'attache aux broches et fils présents à sa position, jamais à distance.

- Label sur une broche ou un fil : nomme le net correspondant.
- Label touchant plusieurs racines non connectées : `AmbiguousLabel`.
- Label hors conducteur : `LabelOffWire`.
- Plusieurs labels sur un net : `MultipleLabelsOnNet` ; le plus petit est retenu.
- Deux nets distincts avec le même texte ne sont pas fusionnés ; le second reçoit
  un suffixe et l'ERC émet `DuplicateNetName`.

L'extraction utilise un union-find : indices `0..P` pour les broches,
`P..P+W` pour les fils.

1. Unir les broches à la même position.
2. Unir chaque broche contenue dans un fil.
3. Classer chaque paire de fils par `segment_relation` : contact ou
   chevauchement colinéaire, croisement avec extrémité, ou croisement intérieur
   seulement avec jonction.
4. Unir les fils contenant une jonction.
5. Rattacher les labels aux racines touchées.
6. Ignorer les groupes sans broche ; leurs fils sont pendants.

Un groupe avec au moins une broche devient un nœud. Broches, fils et labels sont
ordonnés par `(SymbolId, index)`, `WireId` et `NetLabelId`. La complexité actuelle
est quadratique en fils, linéaire par broche/fil, adaptée aux schémas
pédagogiques.

## 5. Noms de nets

L'allocateur réserve `0` à la masse.

- Tous les symboles de masse séparés sont fusionnés dans le même net `0`, même
  sans fil commun ; la masse est donc toujours commune au canevas.
- Une fusion de plusieurs masses émet un `MultipleGround` d'information.
- Le label d'un net de masse est ignoré et émet `GroundLabelIgnored`.
- Un autre net prend son plus petit label.
- Un label déjà pris reçoit `#2`, `#3`, etc., sans dépasser 128 caractères.
- Le label `0` hors masse est renommé et produit `ReservedNetName`.
- Sans label, le net reçoit `N$001`, `N$002`, etc., en évitant les collisions.
- Ordre : masse, puis plus petite broche, plus petit fil, plus petit label,
  racine union-find.

Le nom final est revalidé par `NetName::new`.

## 6. Commandes et undo/redo

Commandes publiques : `AddSymbol`, `RemoveSymbol`, `AddWire`, `RemoveWire`,
`AddJunction`, `RemoveJunction`, `AddLabel`, `RemoveLabel`, `MoveObjects`,
`ReplaceSymbol`, `ReplaceWire`, `ReplaceJunction`, `ReplaceLabel`, `Composite`.

Une suppression stocke l'objet complet et exige qu'il soit identique à l'état
courant, sinon `StaleCommand`. `UndoStack` applique sur un clone, valide, puis
remplace l'état ; un échec laisse tout inchangé. L'historique est illimité, une
nouvelle commande vide le redo, et les identifiants annulés ne sont jamais
réutilisés. Chaque entrée porte un libellé fourni par l'application ;
`undo_to(applied)` ramène l'état au nombre de commandes appliquées demandé, dans
les deux directions.

## 7. Opérations de placement et de câblage

Ces opérations restent des compositions de commandes, donc atomiques et
annulables en une action.

- **Insertion en série** : un composant à deux broches déposé sur un fil
  horizontal ou vertical remplace ce fil par le composant et les tronçons
  restants. Le fil doit mesurer au moins quatre cases et le composant est orienté
  selon l’axe. Une connexion (label ou jonction) située entre ses broches bloque
  l’insertion.
- **Chemin de fil** : un chemin orthogonal peut contenir plusieurs segments. Les
  segments nuls ou déjà présents sont ignorés ; un chemin sans nouveau segment
  est refusé. Un segment colinéaire ajouté au bout d’un fil existant fusionne
  avec lui, sans créer de petit tronçon supplémentaire.
- **Déplacement de segment** : un segment se déplace sur un seul axe, choisi
  selon le glissement dominant : perpendiculairement ou le long de son propre
  axe. La commande ne remplace que ce fil : ses extrémités sont libérées et
  aucun embout ni fil supplémentaire n'est créé. Les broches, labels, jonctions
  et autres fils restent à leur place ; l'ERC signale les connexions perdues.
  Une position déjà occupée par un fil identique est refusée.
- **Déplacement de composant** : les fils reliés aux broches déplacées sont
  regroupés en chaînes simples. Chaque chaîne est retirée puis reconstruite une
  seule fois à travers ses ancres — broches fixes ou déplacées, labels,
  jonctions, contacts en T, extrémités libres — avec un chemin orthogonal. Les
  coudes créés par un déplacement précédent sont ainsi absorbés au lieu de
  s’empiler. Le suivi s’arrête aux branches, aux fils sélectionnés et aux
  chevauchements colinéaires ; un croisement X sans jonction n’est pas une
  connexion. Si la chaîne n’est pas simple ou fiable, elle reste inchangée
  plutôt que de produire une géométrie supplémentaire.
- **Accrochage au déplacement** : si une broche de la sélection arrive à deux
  cases au plus d’un fil ou d’une broche qui ne fait pas partie de son net
  d’origine, le déplacement est ajusté pour l’aligner exactement. Les fils qui
  suivaient la sélection restent alors à leur place : le composant se
  déconnecte de son ancien emplacement. Un composant à deux broches dont les
  deux broches tombent sur le même fil, sans label ni jonction entre elles, y
  est inséré en série, comme lors d’un placement. Un cercle montre la cible
  pendant le glissement. Sans cible, le suivi de chaîne ci-dessus s’applique.
- **Extrémité libre** : une extrémité sans broche, label, jonction ou autre fil
  peut être déplacée sur l’axe du fil ou perpendiculairement. Le déplacement
  perpendiculaire conserve le fil d’origine et ajoute un prolongement avec
  coude ; une extrémité connectée est refusée (`CannotResizeWire`).
- **Sondes** : `Voltmeter`, `Ammeter` et `Ohmmeter` sont des symboles à une
  broche exclus de la netlist. Ils servent de repères de mesure et ne modifient
  ni les nœuds ni les composants simulés.
- **Alignement et distribution** : les objets sélectionnés sont alignés sur une
  arête ou un centre ; la distribution conserve les extrêmes et espace les
  centres.
- **Accroche et routage** : `editor-core::interaction` fournit l’accroche aux
  broches/fils et le routage orthogonal évitant les rectangles de symboles. Ces
  fonctions pures sont testées sans egui.
- **Net** : l’extraction fournit les fils, labels et broches d’un net. La
  sélection de net est déterministe et ne modifie pas la topologie.

## 8. Sélection, presse-papiers et séries

- **Rectangle** : `select_in_rect` compare les bornes de chaque objet à un
  rectangle normalisé. Le mode `Enclosing` exige l’inclusion complète ; le mode
  `Touching` accepte toute intersection. Le changement est `Replace`, `Add` ou
  `Toggle`, dans l’ordre déterministe des `ObjectRef`.
- **Bornes** : un symbole utilise ses broches transformées, élargies d’une case
  pour couvrir son corps ; un fil utilise ses extrémités ; une jonction ou un
  label utilise son point.
- **Presse-papiers interne** : `Clipboard` conserve une copie des objets et une
  ancre au minimum de leurs origines. Le collage décale les positions, alloue de
  nouveaux identifiants, régénère les références de symboles et reconstruit une
  seule commande composite. Un collage refusé ne consomme rien.
- **Rotation et miroir de groupe** : le centre entier de la sélection sert
  d’origine. La rotation applique `(x, y) → (cx + cy − y, cy − cx + x)` et avance
  l’orientation ; le miroir reflète `x` autour de `cx` et inverse le drapeau.
  Fils, jonctions et labels suivent la même transformation.
- **Séries E** : `editor-core::series` fournit E12, E24 et E96, la valeur la plus
  proche et le cycle par pas entier, y compris le changement de décade et le
  signe. La saisie directe reste assurée par `parse_si` (`4k7`, `100n`).

## 9. ERC

Les diagnostics sont triés par sévérité, code, position, sujet et message. Une
erreur ERC n'empêche pas forcément l'extraction ; seule une `ExtractError` la
bloque. Le contrat de simulation reste séparé.

| Code | Sévérité | Sens |
| --- | --- | --- |
| `NoGround` | erreur | aucun symbole de masse |
| `MultipleGround` | information | plusieurs symboles de masse fusionnés en `0` |
| `DuplicateNetName` | erreur | label dupliqué, net renommé |
| `ReservedNetName` | erreur | label `0` hors masse |
| `FloatingPin` | avertissement | broche seule sans fil ni label |
| `StackedPins` | information | plusieurs broches au même point |
| `DanglingWire` | avertissement | fil sans broche |
| `UnconnectedWireEnd` | avertissement | extrémité libre |
| `WireOverlap` | avertissement | chevauchement colinéaire |
| `JunctionOffWire` | avertissement | jonction hors fil |
| `RedundantJunction` | information | jonction inutile |
| `LabelOffWire` | avertissement | label hors conducteur |
| `AmbiguousLabel` | avertissement | label sur plusieurs racines |
| `MultipleLabelsOnNet` | avertissement | plusieurs labels sur un net |
| `GroundLabelIgnored` | information | label ignoré sur la masse |

## 10. JSON et interface

Format versionné :

```json
{
  "schema_version": 1,
  "schematic": {
    "symbols": {},
    "wires": {},
    "junctions": {},
    "labels": {},
    "next_id": 54
  }
}
```

`to_json` valide avant écriture. `from_json` lit `schema_version`, refuse une
version future avant le payload, désérialise puis revalide ; les champs inconnus
sont refusés. `next_id` est persisté, l'historique undo ne l'est pas.

L'interface alloue via `Schematic::allocate_id`, construit les commandes avec les
objets courants, traite `EditError` sans modifier l'état, affiche l'ERC sans
supprimer et redemande une extraction après chaque modification acceptée. Elle ne
modifie jamais directement les `BTreeMap` internes. Une action refusée ne change
ni contenu, ni révision, ni compteur d'identifiants.

## Broches de la référence shunt

Le symbole `ShuntReference` conserve l'ordre `REF`, `K`, `A` lors de l'extraction et de la sérialisation. REF est la broche de mesure, K la cathode de sortie et A l'anode commune ; la tension et le courant principaux affichés sont `V(K)-V(A)` et `I(K vers A)`. Les paramètres sont modifiables dans l'inspecteur.

## Broches du comparateur LM393

Le symbole `Comparator` conserve l'ordre `in+`, `in-`, `out`, `vcc`, `vss`. En orientation `R0`, `in+` et `vcc` sont en haut, `in-` et `vss` en bas. `out` est le collecteur ouvert ; `vss` est le retour du courant sink. Un pull-up externe est requis pour définir le niveau relâché. `vcc` active le modèle entre 1,8 V et 36 V, avec une activation progressive de 1,8 à 2,2 V ; son courant et les limites de mode commun ne sont pas modélisés.

## Broches du MOSFET

Le symbole `Mosfet` conserve l'ordre `d`, `g`, `s` lors de l'extraction, de la
sérialisation et du protocole. Le drain est en haut à droite, la grille à gauche,
la source en bas à droite ; le bulk est interne et relié à la source, sans broche
visible. `swap_ds` ne change pas l'ordre des broches : il permute électriquement
les deux bornes avant l'assemblage et le symbole met en miroir vertical les
liaisons drain/source, la liaison de bulk et la diode de corps, sans déplacer les
points de connexion. La grille ne consomme aucun courant continu et son nœud
reste un nœud électrique normal.

Les paramètres sont créés avec les défauts du registre et restent éditables dans
l'inspecteur ; `channel` y est un choix **Canal N / Canal P**. Un projet sauvegardé
avant l'ajout du MOSFET reste lisible (`SCHEMA_VERSION` inchangé) : seuls les
symboles `Mosfet` portent ces paramètres.
