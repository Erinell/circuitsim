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
- MOSFET : courant de drain `d` vers `s`, tension principale `v(d)-v(s)`, grille
  sans courant continu.
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
I(v) = is * (exp(v / (n * Vt)) - 1) + gmin * v
Vt = kT/q ≈ 25.85 mV à 300.15 K
is = if / (exp(vf / (n * Vt)) - 1)
gmin = 1e-12 S
```

Chaque jonction PN porte en permanence cette conductance shunt `gmin`, comme le
`GMIN` de SPICE. Elle fixe le point de fonctionnement des îlots reliés uniquement
par des jonctions bloquées — par exemple le drain d'un MOSFET bloqué derrière une
LED — sans créer de fuite visible (9,4 pA sous 9,4 V) et sans masquer un nœud
réellement flottant, qui n'a aucune jonction et reste signalé comme singulier.

Le type **Schottky** applique le préréglage `vf = 0.35 V`, `n = 1.05`,
`if = 0.01 A`, `imax = 0.2 A` et `is ≈ 2.5e-8 A` (dérivé). Le type standard
utilise `vf = 0.7 V`, `n = 1`, `is ≈ 1.8e-14 A`. `is` et `n` restent
strictement positifs et les anciennes diodes avec `is`/`n` sont compatibles.
L'exponentielle est plafonnée à `exp(80)` pendant Newton ; ce n'est pas une
résistance série. La LED n'émet aucun flux lumineux modélisé ; sa longueur
d'onde colore uniquement le halo de l'interface. `imax` est une limite de
datasheet éditable, non utilisée par le solveur. La diode n'a ni capacité de
jonction ni température paramétrable ; en AC, seule la conductance `dI/dV + gmin`
au point DC intervient.

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

## 5. Transistor MOSFET à enrichissement

Broches ordonnées : `d` (drain), `g` (grille), `s` (source). Le bulk est relié
à la source à l'intérieur du composant : aucun effet de corps, aucune quatrième
broche. Le modèle est un **niveau 1 carré** (Shichman-Hodges) avec résistances
d'accès et diode de corps.

| Paramètre | Type | Défaut N | Défaut P | Rôle |
| --- | --- | ---: | ---: | --- |
| `channel` | texte | `n` | `p` | canal N ou P |
| `vth` | nombre | `1.6` V | `1.8` V | seuil, strictement positif |
| `k` | nombre | `0.05` A/V² | `0.02` A/V² | gain carré, strictement positif |
| `lambda` | nombre | `0.02` V⁻¹ | `0.02` V⁻¹ | modulation de longueur de canal, ≥ 0 |
| `rd` | nombre | `1.0` Ω | `1.0` Ω | résistance d'accès au drain, ≥ 0 |
| `rs` | nombre | `2.0` Ω | `2.0` Ω | résistance d'accès à la source, ≥ 0 |
| `is` | nombre | `1e-14` A | `1e-14` A | saturation de la diode de corps |
| `n` | nombre | `1.0` | `1.0` | coefficient d'émission de la diode |
| `cgs`, `cgd`, `cds` | nombre | `0` F | `0` F | capacités parasites, ≥ 0 |
| `imax` | nombre | `0.3` A | `0.13` A | courant maximal de datasheet |
| `swap_ds` | booléen | `false` | `false` | permute électriquement D et S |

Les valeurs P sont les valeurs par défaut documentées ; le registre de symboles
ne stocke qu'un seul jeu de défauts (canal N). Pour un canal P, l'inspecteur
change `channel` et l'utilisateur ajuste `vth`/`k`/`imax` selon la fiche.

### Équations

Le modèle travaille en coordonnées de canal. Soit `σ = +1` (N) ou `−1` (P) :

```text
Vgs' = σ * (V(g) - V(s_in))     Vds' = σ * (V(d_in) - V(s_in))
Id'  = σ * Id
Vov  = Vgs' - vth
```

Si `Vov <= 0`, le canal est coupé (`Id' = 0`). Sinon :

```text
régime triode (Vds' < Vov) :
  Id' = k * (2*Vov*Vds' - Vds'^2) * (1 + lambda*Vds')
saturation (Vds' >= Vov) :
  Id' = k * Vov^2 * (1 + lambda*Vds')
```

Pour `Vds' < 0`, le même calcul est appliqué par symétrie en échangeant drain et
source : `Id'(Vgs', Vds') = -Id'(Vgs' - Vds', -Vds')`. La transition à
`Vds' = Vov` est continue (les deux formes coïncident), seule la dérivée change.

Les résistances d'accès sont des inconnues internes : les broches externes sont
reliées aux nœuds internes `d_in` et `s_in` par `rd` et `rs`. Une valeur nulle
supprime le nœud interne correspondant. `swap_ds` est une **permutation
électrique** appliquée avant l'assemblage : le drain du symbole devient la source
interne, et inversement ; le bulk suit la source permutée. Le modèle n'est jamais
dupliqué.

### Diode de corps

La jonction source/bulk vers drain est une diode de la même forme que le
composant `Diode`, orientée source → drain pour le canal N (cathode au drain) et
inversée pour le canal P :

```text
I_bd = is * (exp(-Vds' / (n*Vt)) - 1) + gmin * (-Vds')
Vt ≈ 25.85 mV à 300.15 K
gmin = 1e-12 S
```

Elle conduit donc en polarisation inverse du canal (`Vds' < 0`), comme la diode
parasite réelle. L'exponentielle est plafonnée à `exp(80)`.

### AC

Au point DC, l'AC linéarise `gm = ∂Id'/∂Vgs'` et `gds = ∂Id'/∂Vds'`, ainsi que la
pente de la diode de corps (augmentée du `gmin` de jonction), puis ajoute
`jωcgs`, `jωcgd` et `jωcds`. En régime
saturé, `gm = 2*k*Vov*(1 + lambda*Vds')` et `gds = k*Vov^2*lambda` ; en triode,
`gm = 2*k*Vds'*(1 + lambda*Vds')` et `gds = k*(2*Vov - 2*Vds')*(1 + lambda*Vds')
+ k*(2*Vov*Vds' - Vds'^2)*lambda`. La capacité grille-drain crée le couplage
Miller ; aucune capacité n'est ajoutée si elle vaut zéro.

### Limites

- Pas de tension de seuil dépendante de la température, de la longueur de canal
  ni du bulk (pas d'effet de corps, pas de `vds` de breakdown).
- Pas de régime sous-seuil exponentiel : le courant est exactement nul sous
  `vth`, ce qui rend le « off » idéal.
- `imax` est une limite de datasheet éditable, **non appliquée par le solveur** ;
  elle alimente seulement les alertes de surintensité.
- `rd`/`rs` sont ohmiques, sans dépendance à `Vgs` ; ils ne reproduisent pas la
  courbe `RDS(on)` réelle.
- La diode de corps n'a ni capacité, ni tension de claquage, ni résistance série
  propre.
- La capacité `cds` est linéaire ; il n'y a pas de capacité de recouvrement ni de
  charge de grille.

### Ancrage datasheet

Les défauts N reprennent l'ordre de grandeur du **2N7002** (Nexperia) :
`VGS(th) = 1.0…2.5 V`, `ID = 300 mA`, `RDS(on) ≈ 1…1.6 Ω` à `VGS = 10 V`. Le
`vth = 1.6 V` est la valeur typique des variantes 2N7002BK/HW, `imax = 0.3 A` le
courant continu, et `k = 0.05 A/V²` redonne `1/(2*k*Vov) ≈ 1.2 Ω` au point de
mesure `VGS = 10 V`. Source : [Nexperia, fiche 2N7002](https://assets.nexperia.com/documents/data-sheet/2N7002.pdf)
et [2N7002BK](https://assets.nexperia.com/documents/data-sheet/2N7002BK.pdf).

Les défauts P reprennent le **BSS84** (onsemi) : `VGS(th) = −0.8…−2.0 V`,
`ID = −0.13 A`, `RDS(on) ≤ 10 Ω` à `VGS = −5 V`. `vth = 1.8 V` et
`imax = 0.13 A` viennent de la fiche ; `k = 0.02 A/V²` redonne `≈ 7.8 Ω` à
`VGS = −5 V`. Source : [onsemi, fiche BSS84](https://www.onsemi.com/pdf/datasheet/bss84-d.pdf).

## 6. Référence shunt programmable TL431/LM431

Le composant possède trois broches, dans l'ordre stable du symbole, de la netlist et du protocole : `REF`, `K` (cathode), `A` (anode). Son courant principal `I_KA` est orienté de `K` vers `A` et sa tension principale vaut `V(K)-V(A)`. Le courant d'entrée `I_REF` est orienté de `REF` vers `A`. La broche REF reçoit le réseau externe de programmation ; avec un diviseur `Rhaut` de K à REF et `Rbas` de REF à A, la consigne idéale vaut `V_KA ≈ VREF * (1 + Rhaut/Rbas) + IREF * Rhaut`.

| Paramètre | Défaut | Domaine / rôle |
| --- | ---: | --- |
| `vref` | `2.495` V | strictement positif ; consigne interne |
| `iref` | `2e-6` A | non négatif ; courant constant de REF vers A |
| `ik_min` | `1e-3` A | strictement positif ; courant cathode au point de consigne |
| `imax` | `0.1` A | courant cathode plafond du modèle, `imax >= ik_min` |
| `z_ka` | `0.2` Ω | strictement positif ; pente de contrôle et impédance incrémentale K-A avec REF reliée à K |

Loi statique bornée du solveur :

```text
e = V(REF) - V(A) - vref
Icmd = clamp(ik_min + e/z_ka, 0, imax)
x = clamp((V(K)-V(A))/vref, 0, 1)
h = x^2 * (3 - 2x)
I_KA = Icmd * h
I_REF = iref  (courant de REF vers A)
```

`h` est une transition lisse de conformité K-A : le composant ne débite pas en sens inverse et le courant augmente progressivement lorsque `VKA` passe de 0 à `vref`. En régulation, hors saturation et avec `VKA >= vref`, la pente vaut `dI_KA/de = 1/z_ka` ; si REF est reliée à K, `z_ka` reproduit l'impédance incrémentale déclarée. Si le courant demandé est inférieur à `ik_min` ou dépasse `imax`, le modèle cesse de réguler ; `imax` est une saturation du modèle, pas une garantie de fonctionnement continu.

Les valeurs par défaut reprennent les valeurs usuelles de la fiche **TI TL431, TL432 Precision Programmable Reference** : `VREF = 2.495 V` typique, `IREF = 2 µA` typique, courant cathode de 1 à 100 mA et `|zKA| = 0.2 Ω` typique. Source : [Texas Instruments, fiche TL431/TL432](https://www.ti.com/lit/ds/symlink/tl431.pdf), caractéristiques électriques et rubriques *Features* / *Dynamic Impedance*.

Modèle volontairement simplifié : commande instantanée sans pôle ni compensation, capacité, bruit, tolérance, température, protection thermique ou dynamique de démarrage. `IREF` est constant et la transition de conformité est une interpolation numérique, non une caractéristique transistor précise. DC et transitoire utilisent la même loi algébrique ; l'AC linéarise ses dérivées au point DC, sans introduire de pôle.

## 7. Comparateur à collecteur ouvert LM393

Broches ordonnées : `in+`, `in-`, `out`, `vcc`, `vss`. Le courant de sortie positif est orienté `out -> vss`, et la tension observée est `V(out)-V(vss)`. Le LM393 a une sortie collecteur ouvert : **le pull-up est externe et obligatoire** pour obtenir un niveau haut. Sans pull-up, un état relâché laisse le nœud de sortie flottant et la résolution DC peut être singulière. `vss` sert de référence à l'étage de sortie ; `vcc` autorise la comparaison si `2 V <= V(vcc)-V(vss) <= 36 V`.

| Paramètre | Défaut | Domaine / rôle |
| --- | ---: | --- |
| `v_offset` | `2e-3` V | offset d'entrée ; `-5e-3 <= v_offset <= 5e-3` V |
| `r_sink` | `20` Ω | strictement positif ; résistance de sortie lorsque le collecteur conduit |
| `imax` | `20e-3` A | strictement positif et au plus 20 mA ; plafond du courant de sink |

```text
e = V(in+) - V(in-) - v_offset
si V(vcc)-V(vss) hors [2 V, 36 V] : collecteur relâché
sinon si e < 0 : Iout = min((V(out)-V(vss))/r_sink, imax)
sinon    : Iout = 0  (collecteur relâché)
```

Le courant `Iout` positif va de `out` à `vss`. Dans la plage normale `V(out) >= V(vss)`, l'état conducteur se comporte comme `r_sink` jusqu'au plafond `imax`. En dehors de cette plage, la résistance est prolongée algébriquement pour aider Newton ; le modèle ne représente pas la protection contre une tension de sortie négative. Un pull-up résistif donne, en état bas non saturé, `Vout-VSS ≈ r_sink * Iout`.

Le défaut `v_offset = 2 mV` reprend l'ordre de grandeur typique de la fiche LM393 ; ±5 mV est la limite spécifiée. `r_sink = 20 Ω` approxime le point typique `VOL ≈ 80 mV` à `IOL = 4 mA` de la courbe de sortie. `imax = 20 mA` correspond à la valeur absolue maximale de courant de sortie, pas à un courant de fonctionnement recommandé. Source : [Texas Instruments, fiche LM393/LM393B](https://www.ti.com/lit/ds/symlink/lm393.pdf), sections 5.1, 5.8 et 5.12, et [fiche produit LM393](https://www.ti.com/product/LM393).

Modèle volontairement simplifié : entrée sans courant, courant d'alimentation nul, limites de mode commun non modélisées, commutation instantanée, sans délai ni hystérésis. L'AC fige l'état logique au point DC et linéarise seulement la résistance de sink active ; le gain de comparaison petit-signal est nul. La résistance de pull-up et sa source restent des composants externes.

## 8. Amplificateur opérationnel

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

## 9. Sources et excitations

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

## 10. Composants personnalisés

`ComponentKind::Custom(String)` et `SymbolKind::Custom(String)` représentent une
extension. Le registre valide nom, broches et paramètres. Les solveurs refusent
explicitement le type avec `MnaError::UnsupportedComponent` tant qu'aucun modèle
n'est implémenté. L'exemple `examples/custom_mosfet.rs` montre la représentation,
l'extraction et le JSON ; il ne s'agit pas d'un transistor simulé.

## 11. Énergie

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
