# Unités et netlist

`sim-core` définit les unités SI, le modèle de netlist et les validations
partagées par l'éditeur, le worker et les solveurs. Les modèles électriques sont
dans [COMPONENTS.md](COMPONENTS.md), les API dans [SIMULATION.md](SIMULATION.md).

## 1. Unités

| Type | Grandeur | Symbole | Exemple |
| --- | --- | --- | --- |
| `Volts` | tension | `V` | `5 V` |
| `Amps` | courant | `A` | `1 mA` |
| `Ohms` | résistance | `Ω` | `4.7 kΩ` |
| `Farads` | capacité | `F` | `1 µF` |
| `Henrys` | inductance | `H` | `1 mH` |
| `Seconds` | temps | `s` | `1 ms` |
| `Hertz` | fréquence | `Hz` | `1 MHz` |
| `Watts` | puissance | `W` | `500 mW` |
| `Coulombs` | charge | `C` | `2.2 µC` |
| `Celsius` | température | `°C` | `25 °C` |

Les valeurs sont stockées en SI. `Celsius` est la seule conversion explicite :
`from_kelvin(300.0) = 26.85 °C`, `to_kelvin` ajoute `273.15`. Le trait `Si`
expose `UNIT`, `from_si`, `to_si`, `format` et `parse`. Les opérations acceptent
`+`, `-`, `*` et `/` par `f64`, ainsi que la négation ; les unités différentes ne
s'additionnent pas.

## 2. Parsing et formatage

`parse_si` accepte un nombre avec ou sans suffixe ; `parse_quantity::<Q>` vérifie
que l'unité correspond. Formes acceptées : `4700`, `4.7`, `1e-3`, `4.7k`, `2u2`,
`3M3`, `4.7kΩ`, `2.2uF`, `1MHz`, `25°C`, `1kohm`.

| Préfixe | Facteur | Préfixe | Facteur |
| --- | ---: | --- | ---: |
| `p` | `1e-12` | `k`, `K` | `1e3` |
| `n` | `1e-9` | `M` | `1e6` |
| `u`, `µ` | `1e-6` | `G` | `1e9` |
| `m` | `1e-3` | `T` | `1e12` |

`m` signifie milli et `M` méga. La notation SPICE `4k7` vaut `4700`, `2u2` vaut
`2.2e-6` ; `ohm` et `Ohm` sont acceptés. Erreurs : `Empty`, `InvalidNumber`,
`MismatchedUnit`, `NotFinite` (`NaN`), `Overflow` (infini ou hors `f64`). Aucune
conversion silencieuse entre unités.

`format_si` utilise quatre chiffres significatifs et un préfixe d'ingénierie
multiple de trois, de pico à téra : `4.7 kΩ`, `1 mA`, `1 µF`, `1 kV` pour
`999.95 V`. Les zéros finaux sont supprimés ; `Celsius` n'utilise pas de préfixe.
Les valeurs non finies sont affichées `NaN` ou `inf` avec leur unité.

## 3. Modèle

```text
Netlist
  nodes:       Vec<Node>
  components:  Vec<ComponentSpec>

Node
  id: NodeId
  name: NetName
  kind: Ground | Electrical | Internal { owner, index }

ComponentSpec
  id: ComponentId
  kind: ComponentKind
  label: String
  params: BTreeMap<String, ParamValue>
  pins: Vec<NodeId>
```

`NodeId` et `ComponentId` sont des `u32` et, dans une netlist validée, égalent
l'indice du vecteur. `NetName` coupe les espaces, exige un nom non vide, sans
caractère de contrôle et de 128 caractères au plus ; il fournit
`NetName::ground()` et les noms générés `N$001`. `ParamValue` distingue
`Number(f64)`, `Bool(bool)` et `Text(String)`.

## 4. Composants

| `ComponentKind` | Broches | Ordre |
| --- | ---: | --- |
| `Resistor`, `Capacitor`, `Inductor` | 2 | `a`, `b` |
| `VoltageSource`, `CurrentSource`, `Switch` | 2 | `a`, `b` |
| `Diode`, `Led`, `Zener` | 2 | `a`, `b` |
| `Ground` | 1 | `gnd` |
| `OpAmp` | 5 | `in+`, `in-`, `out`, `v+`, `v-` |
| `Custom(String)` | ≥ 1 | défini par le registre |

Les paramètres, valeurs par défaut et modèles sont dans
[COMPONENTS.md](COMPONENTS.md). La netlist transporte les nœuds dans l'ordre du
vecteur `pins`, pas les noms de broches. Les excitations AC ne font pas partie de
la netlist : elles vivent dans `AcExcitation`.

`Ground` reste un composant pour l'éditeur et l'extraction, avec une broche sur
le nœud de masse ; les solveurs l'ignorent ensuite. `Custom` est accepté par la
netlist mais refusé par les solveurs tant qu'aucun modèle n'existe.

## 5. Validation

`Netlist::new` et `validate` vérifient :

- identifiants de nœuds et de composants uniques et continus ;
- noms de nœuds uniques et valides ; labels de composants uniques après trim ;
- noms de types personnalisés valides ;
- au moins une broche par composant et nombre de broches attendu pour les types
  intégrés ;
- références de broches vers des nœuds existants ;
- noms de paramètres valides et nombres finis ;
- composant `Ground` relié à un nœud `Ground` ;
- propriétaire existant et unicité de `(owner, index)` pour les nœuds internes.

`validate` accepte zéro ou plusieurs masses ; une netlist structurellement
valide peut donc être incomplète. `Netlist::empty()` est valide mais non
simulable.

`validate_for_simulation` ajoute exactement un nœud `Ground` nommé `0`
(`NoGround`, `MultipleGround`, `InvalidGroundName`). La masse n'est pas
nécessairement le premier nœud et est exclue des inconnues.

## 6. Construction et sérialisation

`NetlistBuilder` alloue les identifiants dans l'ordre d'ajout, valide
immédiatement noms et labels, puis `build()` applique `Netlist::new`. Une erreur
d'ajout n'ajoute rien ; une erreur de `build` ne produit pas de netlist partielle.

`Netlist`, `Node`, `ComponentSpec` et `ParamValue` implémentent Serde. La netlist
brute est interne et non versionnée. Le document utilisateur versionné est le
JSON de schéma (`SCHEMA_VERSION = 1`, voir [EDITOR.md](EDITOR.md)). Le protocole
worker a sa propre représentation et son propre versionnage
([PROTOCOL.md](PROTOCOL.md)).

`extract_netlist` transforme symboles, groupes de connectivité et labels en
composants, nœuds et noms, et produit aussi `ExtractedNet`, `net_of_pin` et
`ErcReport`. Une netlist extraite doit encore passer `validate_for_simulation`.
