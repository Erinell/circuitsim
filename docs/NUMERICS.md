# Noyau numérique

Le cœur assemble une MNA dense et la résout par LU. Il est déterministe,
compatible WASM et sans horloge. Les modèles électriques sont dans
[COMPONENTS.md](COMPONENTS.md), les API et options dans [SIMULATION.md](SIMULATION.md).

## 1. Inconnues

Pour une netlist validée :

- `n` nœuds électriques hors masse ;
- `m` inconnues de branche : sources de tension, inductances, sorties d'opamp ;
- système de taille `n + m`, masse sans ligne ni colonne ;
- `x = [v_1 ... v_n, i_1 ... i_m]^T`, avec `i` orienté `a -> b`.

Les nœuds internes sont des nœuds électriques normaux.

## 2. Assemblage DC

Système `G x = b`, contributions ajoutées dans l'ordre des composants.

### Résistance et commutateur

Avec `g = 1/R` :

```text
G[a,a] += g    G[a,b] -= g
G[b,a] -= g    G[b,b] += g
```

`R = ron` si `closed`, sinon `roff`. Deux broches sur le même nœud ajoutent zéro.

### Source de courant

```text
b[a] -= I      b[b] += I
```

### Source de tension et inductance

Pour une source `E`, ou `E = 0` pour une inductance en DC :

```text
KCL a : +i          KCL b : -i
branche : v_a - v_b = E
G[a,k] += 1    G[b,k] -= 1
G[k,a] += 1    G[k,b] -= 1
b[k] += E
```

Une branche avec deux broches identiques est refusée (`DegenerateBranch`).

### Condensateur et inductance en DC

Le condensateur est ouvert (aucune contribution, aucune inconnue). L'inductance
est une branche de tension nulle avec inconnue de courant.

## 3. Assemblage transitoire

Euler arrière avec `h > 0` et la solution précédente comme point de départ.

### Condensateur

```text
i_C = C/h * (v_ab,n - v_ab,n-1)
```

Conductance `C/h` entre `a` et `b`, plus une source équivalente
`-C/h * v_ab,n-1` de `a` vers `b`.

### Inductance

Avec courant `i` de `a` vers `b` :

```text
v_ab = L/h * (i_n - i_n-1)
v_ab - L/h * i_n = -L/h * i_n-1
```

La ligne de branche reçoit les coefficients de tension `a`/`b`, `-L/h` sur son
courant et `-L/h * i_n-1` au second membre. KCL reste `+i` en `a`, `-i` en `b`.

Sources et commutateurs gardent leur modèle DC pendant l'intervalle ; leurs
valeurs changent aux événements, après l'échantillon correspondant.

## 4. Linéarisation non linéaire

Pour `I(v)` et `g = dI/dv` :

```text
I(v) ≈ g v + I_eq
I_eq = I(v0) - g v0
G[a,a] += g    G[a,b] -= g
G[b,a] -= g    G[b,b] += g
b[a] -= I_eq   b[b] += I_eq
```

Les expressions de `I` et `g` des diodes, LED, Zener et de l'opamp sont dans
[COMPONENTS.md](COMPONENTS.md). L'opamp est un VCVS saturé : sa branche de sortie
impose `v(out) - f(d) = 0`, linéarisée avec `f'(d) = gain` en actif et `0` en
saturation. `v+` et `v-` ne participent pas aux équations.

## 5. Newton et robustesse

L'état initial DC est `x = 0`, tensions de jonction à zéro. Pour un pas
transitoire, l'état initial est la solution précédente. À chaque itération :

1. calculer la tension cible des jonctions ;
2. la limiter par rapport à l'état précédent ;
3. assembler avec les tensions limitées ;
4. résoudre par LU ;
5. évaluer le résidu réel ;
6. réduire éventuellement le pas par recherche linéaire ;
7. comparer mise à jour et résidu aux tolérances.

Limitation logarithmique :

```text
si cible > précédent :
  limite = précédent + Vt_eff * ln(1 + (cible - précédent)/Vt_eff)
si cible < précédent :
  limite = précédent - Vt_eff * ln(1 + (précédent - cible)/Vt_eff)
```

`Vt_eff = n*Vt` pour une diode et `max(n, nbv)*Vt` pour un Zener. Elle évite
qu'une jonction à `0 V` soit évaluée directement à `5 V`.

Acceptation :

```text
max|x_new - x_old| <= abs_tolerance + rel_tolerance * max|x_new|
max|G(x_new) x_new - b(x_new)| <= residual_tolerance
```

Le résidu utilise les tensions réelles, pas limitées. Une analyse linéaire non
vide s'arrête après une résolution ; un système sans inconnue retourne 0
itération.

### Continuations

- `Plain` : Newton simple.
- `Gmin` : fuites décroissantes `1e-3, 1e-4, 1e-5, 1e-6, 1e-8, 1e-10, 0` de
  chaque nœud à la masse.
- `SourceStepping` : sources multipliées par `0, 0.25, 0.5, 0.75, 1`.
- `Auto` : Newton, puis `gmin`, puis source stepping.

Les étapes intermédiaires ne servent qu'à initialiser. La dernière étape de
chaque continuation est toujours résolue avec `gmin = 0` et les sources
nominales, donc une singularité exacte reste une erreur.

## 6. Résolution linéaire

`nalgebra::DMatrix::lu().solve(..)` avec pivotage partiel détecte les matrices
singulières exactes. Une solution `NaN`/`inf` produit `NonFiniteSolution`.
L'AC utilise la même LU sur `DMatrix<Complex<f64>>`.

Le choix dense est adapté aux schémas pédagogiques : assemblage simple, tests
aisés, WASM sans rayon, complexité `O((n+m)^3)` acceptable. `faer` reste approuvé
pour les systèmes plus grands ; aucun solveur creux ou itératif n'est branché.

## 7. Analyse AC

Le point DC est calculé avec les équations DC. Pour chaque `f > 0`,
`ω = 2πf` :

```text
Y(jω) x_ac = b_ac
x_ac = [v_1 ... v_n, i_1 ... i_m]^T
```

Contributions :

- résistance/commutateur : conductance réelle ;
- condensateur : `jωC` entre ses broches ;
- inductance : `v(a)-v(b)-jωL i = 0` ;
- source de tension : phasor sur sa branche ;
- source de courant : injection du phasor de `a` vers `b` ;
- diode/LED/Zener : conductance `dI/dV` au DC, sans capacité ;
- opamp : pente `gain` en actif, `0` en saturation ;
- masse : `0 V`.

Une source absente a un phasor nul (court-circuit ou circuit ouvert petits
signaux selon son type). Le `dc` n'est jamais une amplitude AC. Les règles de
balayage sont dans [SIMULATION.md](SIMULATION.md). Une fréquence en échec annule
le résultat complet. Les phasors sont des amplitudes crêtes ; `|V| exp(jφ)`, φ en
radians dans le résultat et degrés dans les excitations.

## 8. Énergie et déterminisme

Pour chaque intervalle, la puissance est évaluée à `x_{n+1}` et multipliée par
`h`. Sources et sortie d'opamp fournissent une énergie positive si leur puissance
absorbée est négative ; résistances, commutateurs et jonctions dissipent.

```text
E_C = 1/2 * C * v_ab^2
E_L = 1/2 * L * i^2
residual = supplied - dissipated - stored_delta
```

Le résidu est diagnostique. La puissance de fin de pas et les modèles sans
résistance série empêchent une conservation exacte.

Le déterminisme repose sur l'ordre des composants, l'allocation des branches,
le tri des événements, la génération AC croissante, les `BTreeMap` et la LU
déterministe. Aucune graine, horloge ou table de hachage n'intervient. Une même
entrée produit les mêmes valeurs, clés et échantillons.

Tolérances et erreurs sont dans [SIMULATION.md](SIMULATION.md). Une
non-convergence ne produit pas de solution partielle. Les FFT et Bode sont des
traitements de présentation détaillés dans [INSTRUMENTS.md](INSTRUMENTS.md).

## 9. Limites

- Euler arrière du premier ordre, sans pas adaptatif ni erreur locale.
- Excitations continues représentées par des événements discrets.
- AC petits signaux sans capacité de jonction, résistance série de diode ni
  température paramétrable.
- Opamp sans alimentation, limite de courant ni pôle AC ; pente `gain` ou `0`.
- Résidu DC en unités mixtes A/V ; heuristiques de convergence non globales.
- Solveur dense ; grands schémas à factoriser autrement.
