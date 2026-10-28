# Composants et modèles

Ce document décrit les paramètres, valeurs par défaut et modèles électriques.
Les équations d'assemblage MNA sont dans [NUMERICS.md](NUMERICS.md), les API dans
[SIMULATION.md](SIMULATION.md). Les paramètres sont numériques finis et stockés
en SI, sauf `closed` qui est booléen.

## 1. Broches et signes

Les composants à deux broches utilisent `a`, `b`. L'opamp utilise `in+`, `in-`,
`out`, `v+`, `v-` dans cet ordre ; `Ground` a une broche `gnd` sur la masse.

- Tension : `v(a) - v(b)`.
- Courant de branche : de `a` vers `b`.
- Source de courant : `dc` de `a` vers `b` à travers la source.
- Diode : anode `a` vers cathode `b`.
- Opamp : sortie référencée à la masse, courant `out -> masse`.
- Condensateur : `i = C dv/dt` de `a` vers `b` ; inductance :
  `v = L di/dt`, courant `a -> b`.

Une source de courant positive retire du courant en `a` et en injecte en `b`.
Une source de tension qui fournit de la puissance a souvent un courant de branche
négatif. En AC, `AcExcitation.magnitude` est une amplitude crête et
`phase_degrees` est en degrés.

## 2. Composants linéaires

| Composant | Paramètre (défaut) | DC | Transitoire | AC |
| --- | --- | --- | --- | --- |
| `Resistor` | `resistance = 1000` Ω, `imax = 0.25` A | `i = v/R` | `i = v/R` | conductance `1/R` |
| `Capacitor` | `capacitance = 1e-6` F, `imax = 1` A | ouvert | `i = C dv/dt`, Euler arrière | `jωC` |
| `Inductor` | `inductance = 1e-3` H, `resistance = 0.01` Ω, `imax = 0.5` A | `v = R i` (court-circuit si `R = 0`) | `v = R i + L di/dt` | `R + jωL` |
| `VoltageSource` | `dc = 5` V, `imax = 10` A | `v(a)-v(b)=dc` | valeur après événements | phasor |
| `CurrentSource` | `dc = 1e-3` A, `imax = 1` A | `dc` de `a` vers `b` | valeur après événements | phasor |
| `Ground` | aucun | `0 V` | `0 V` | `0 V` |

`resistance`, `capacitance` et `inductance` doivent être strictement positives ;
la résistance série d’une inductance peut être nulle. Une source peut être nulle
ou négative, mais finie. Une source de tension ou une inductance avec deux
broches identiques est refusée (`DegenerateBranch`). `imax` est une limite
d’affichage et de diagnostic, jamais utilisée par le solveur ; une valeur nulle
ou absente désactive l’alerte.

En DC, `C` est ouvert et `L` se réduit à sa résistance série. En transitoire, `C`
devient une conductance `C/h` avec source équivalente et `L` conserve son courant
avec sa résistance. En AC, `C` devient `jωC` et `L` vaut `R + jωL`.

## 3. Commutateur

| Paramètre | Type | Défaut | Rôle |
| --- | --- | --- | --- |
| `ron` | nombre | `1e-3` Ω | résistance fermée |
| `roff` | nombre | `1e18` Ω | résistance ouverte |
| `imax` | nombre | `1` A | courant maximal d'affichage |
| `closed` | booléen | `false` | état logique |

Le solveur choisit `ron` si `closed`, sinon `roff` ; pas d'hystérésis ni de
tension de commande. Un `TransientAction::SetSwitchClosed` change l'état après
l'échantillon à l'instant exact. `closed` absent produit `MissingParameter`,
mauvais type `InvalidParameterType` ; l'ancien paramètre `threshold` n'est pas
interprété.

## 4. Diode, LED et Zener

| Paramètre | Défaut diode | Défaut LED | Rôle |
| --- | ---: | ---: | --- |
| `type` | `standard` | — | `standard` ou `schottky` |
| `is` | dérivé de `vf`/`if` | dérivé de `vf`/`if` | courant de saturation |
| `n` | `1` | `2` (implicite) | coefficient d'émission |
| `wavelength` | — | `620` nm | couleur / longueur d'onde |
| `vf` | `0.7` V | `2.0` V | tension directe au courant nominal |
| `if` | `0.01` A | `0.02` A | courant nominal |
| `imax` | `0.2` A | `0.03` A | courant maximal |

```text
I(v) = is * (exp(v / (n * Vt)) - 1)
Vt = kT/q ≈ 25.85 mV à 300.15 K
is = if / (exp(vf / (n * Vt)) - 1)
```

Le type **Schottky** applique le préréglage `vf = 0.35 V`, `n = 1.05`,
`if = 0.01 A`, `imax = 0.2 A` et `is ≈ 2.5e-8 A` (dérivé). Le type standard
utilise `vf = 0.7 V`, `n = 1`, `is ≈ 1.8e-14 A`. `is` et `n` restent
strictement positifs et les anciennes diodes avec `is`/`n` sont compatibles.
L'exponentielle est plafonnée à `exp(80)` pendant Newton ; ce n'est pas une
résistance série. La LED n'émet aucun flux lumineux modélisé ; sa longueur
d'onde colore uniquement le halo de l'interface. `imax` est une limite de
datasheet éditable, non utilisée par le solveur. La diode n'a ni capacité de
jonction ni température paramétrable ; en AC, seule la conductance `dI/dV` au
point DC intervient.

Le Zener ajoute, pour `v < -bv` :

```text
I_bd(v) = -ibv * (exp((-v - bv) / (nbv * Vt)) - 1)
I(v) = I_direct(v) + I_bd(v)
```

Le Zener réutilise `is = 1e-14` A et `n = 1` pour le terme direct. `bv` (défaut
`5.1` V), `ibv` (`1e-3` A), `nbv` (`1`) et `imax` (`0.2` A) sont strictement
positifs. Le terme est nul au seuil et devient négatif au-delà. La pente AC
inclut le claquage ; résistance dynamique indépendante, bruit, capacité et
température ne sont pas modélisés.

## 5. Amplificateur opérationnel

| Paramètre | Défaut | Rôle |
| --- | ---: | --- |
| `gain` | `1e5` | gain différentiel fini, > 0 |
| `v_sat_pos` | `15` V | saturation haute |
| `v_sat_neg` | `-15` V | saturation basse |

```text
v(out) = clamp(gain * (v(in+) - v(in-)), v_sat_neg, v_sat_pos)
```

La sortie a une inconnue de courant `out -> masse`. `v+` et `v-` sont déclarées
pour le symbole mais ignorées : aucun courant d'alimentation, limite de courant,
résistance de sortie, slew rate ni pôle. Une alimentation flottante peut rendre
le système singulier. Newton utilise `gain` en régime actif et `0` en saturation ;
l'AC réutilise cette pente. `v_sat_pos > v_sat_neg`, sinon
`InvalidSaturation`.

## 6. Sources et excitations

Une source de tension ou de courant est décrite au repos par `dc`. En
transitoire :

```rust
TransientAction::SetSourceValue { component, value }
```

La cible doit être une source et la valeur finie. L'événement s'applique après
l'échantillon à son instant ; l'intervalle suivant utilise la nouvelle valeur. Les
formes d'onde continues (sinusoïde, rampe, PWM) ne sont pas paramétrées : elles
se représentent par des événements.

En AC, l'excitation est portée par
`AcExcitation { component, magnitude, phase_degrees }` :

- seules les sources de tension et de courant sont acceptées ;
- une source absente a un phasor nul ;
- un doublon est refusé ;
- `magnitude` est une amplitude crête finie et non négative ; `phase_degrees`
  est fini.

`dc` ne sert qu'au point de fonctionnement. Une source de tension AC nulle est un
court-circuit petits signaux ; une source de courant AC nulle est un circuit
ouvert. L'orientation AC reste `a -> b`.

## 7. Composants personnalisés

`ComponentKind::Custom(String)` et `SymbolKind::Custom(String)` représentent une
extension. Le registre valide nom, broches et paramètres. Les solveurs refusent
explicitement le type avec `MnaError::UnsupportedComponent` tant qu'aucun modèle
n'est implémenté. L'exemple `examples/custom_mosfet.rs` montre la représentation,
l'extraction et le JSON ; il ne s'agit pas d'un transistor simulé.

## 8. Énergie

L'audit transitoire évalue la puissance à la fin de chaque pas et calcule :

```text
E_C = 1/2 * C * v_ab^2
E_L = 1/2 * L * i^2
residual = supplied - dissipated - stored_delta
```

Les sources fournissent une énergie positive lorsque leur puissance absorbée est
négative. Les alimentations de l'opamp ne sont pas comptées, seule sa sortie. Le
résidu est diagnostique : puissance de fin de pas et modèles simplifiés
expliquent qu'il ne soit pas exactement nul.
