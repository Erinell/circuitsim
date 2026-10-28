# Simulation DC, transitoire et AC

`sim-core` expose trois analyses MNA : point de fonctionnement DC, transitoire à
pas fixe et réponse petits signaux AC. Une analyse dépend uniquement d'une
netlist validée et de ses options ; elle compile pour WASM et ne lit aucune
horloge. Les modèles sont dans [COMPONENTS.md](COMPONENTS.md), les équations dans
[NUMERICS.md](NUMERICS.md).

## 1. Préconditions et résultats

Les trois solveurs exigent `validate_for_simulation` : exactement un nœud
`Ground` nommé `0`. Ils revalident la netlist. Une erreur ne produit jamais de
solution partielle présentée comme valide.

Tous les résultats sont en SI et dans des `BTreeMap`. La masse vaut `0 V` ;
`branch_currents` ne contient que les composants à inconnue de courant : sources
de tension, inductances et sorties d'opamp.

## 2. API DC

```rust
use sim_core::{solve_dc, DcOptions};

let result = solve_dc(&netlist, DcOptions::default())?;
let vout = result.node_voltage(output_node);
let current = result.branch_current(source_component);
```

| Type | Contenu |
| --- | --- |
| `DcOptions` | itérations, tolérances, stratégie Newton |
| `DcResult` | tensions de tous les nœuds, courants de branche, itérations, résidu |
| `DcError` | options, compilation, singularité, valeur non finie, non-convergence |
| `MnaError` | composant/paramètre invalide, saturation, branche dégénérée, netlist |

`iterations` compte les systèmes linéaires résolus, continuations comprises ;
une analyse linéaire non vide retourne `1`, une netlist sans inconnue `0`.
`residual` est le résidu maximal en unités mixtes A/V.

### Convergence

| Option | Défaut | Rôle |
| --- | ---: | --- |
| `max_iterations` | `100` | systèmes résolus par tentative |
| `abs_tolerance` | `1e-10` | tolérance absolue de mise à jour |
| `rel_tolerance` | `1e-7` | tolérance relative de mise à jour |
| `residual_tolerance` | `1e-7` | résidu maximal |
| `strategy` | `Auto` | `Plain`, `Gmin`, `SourceStepping`, `Auto` |

Une option non finie, un maximum nul ou une tolérance négative produit
`InvalidOptions`. À chaque itération, le solveur limite les tensions de jonction,
résout par LU, évalue le résidu réel, peut amortir par recherche linéaire et
vérifie mise à jour + résidu.

`Auto` tente Newton simple, puis une continuation `gmin` décroissante, puis un
source stepping. La dernière solution de chaque continuation est recalculée avec
`gmin = 0` et les sources nominales ; une singularité exacte n'est pas masquée.
`Plain` désactive les reprises ; `Gmin` et `SourceStepping` les imposent.

## 3. Conventions de signe

Le tableau complet des orientations est dans [COMPONENTS.md](COMPONENTS.md).
Les règles structurantes sont : tension `v(a)-v(b)`, courant de branche `a -> b`,
source de courant `dc` de `a` vers `b` à travers la source, et masse à `0 V`. Une
source positive retire du courant en `a` et en injecte en `b` ; un phasor AC est
`V = |V| exp(jφ)`, avec φ en radians dans le résultat. Les excitations AC suivent
la même orientation que le DC.

## 4. API transitoire

```rust
use sim_core::{simulate_transient, TransientAction, TransientEvent, TransientOptions};

let options = TransientOptions {
    start_time: 0.0,
    stop_time: 2e-3,
    step: 1e-4,
    events: vec![TransientEvent {
        time: 0.0,
        action: TransientAction::SetSourceValue { component: source, value: 5.0 },
    }],
    ..TransientOptions::default()
};
let result = simulate_transient(&netlist, options)?;
```

| Type | Contenu |
| --- | --- |
| `TransientOptions` | intervalle, pas, `max_steps`, `DcOptions`, événements |
| `TransientEvent` / `TransientAction` | source ou commutateur daté |
| `TransientSample` | instant, tensions, courants de branche |
| `TransientResult` | échantillons, audit d'énergie, pas, itérations |
| `TransientError` | options/événements, point initial, pas, limite |

Défauts : `start_time = 0`, `stop_time = 1 ms`, `step = 10 µs`,
`max_steps = 1_000_000`, aucun événement.

Le point initial du transitoire force les condensateurs à **tension nulle**
(charge complètement déchargée) et court-circuite les inductances, comme en DC.
Le calcul DC classique reste condensateur ouvert. Si le point initial échoue,
`InitialDc` ; aucun échantillon partiel.
Chaque intervalle `[t_n, t_{n+1}]` utilise Euler arrière avec `h` :

```text
C dv/dt ≈ C/h * (v_{n+1} - v_n)
L di/dt ≈ L/h * (i_{n+1} - i_n)
```

La chronologie contient début, instants nominaux et événements. Les pas nuls ou
confondus avec un événement sont fusionnés ; les bornes et événements restent
présents. Un événement doit être dans `[start_time, stop_time)` ; les doublons
même instant + composant sont refusés.

Un événement s'applique **après** l'échantillon à son instant : l'échantillon
décrit l'état avant, l'intervalle suivant le nouvel état. Un événement à
`start_time` modélise un échelon au démarrage. Le résultat contient
`steps + 1` échantillons ; `iterations` totalise les systèmes Newton, continuations
comprises ; `max_steps` borne les intervalles nominaux et supplémentaires.

## 5. Énergie transitoire

`TransientResult::energy` fournit `supplied`, `dissipated`, `stored_delta`,
`residual = supplied - dissipated - stored_delta`, et `components` avec
`ComponentEnergy` par composant. La puissance est évaluée en fin de pas ; la
variation stockée est calculée à partir des tensions/courants finaux. Le résidu
est diagnostique. Les alimentations de l'opamp sont exclues, seule sa sortie
compte. Formules et limites : [NUMERICS.md](NUMERICS.md).

## 6. API AC

```rust
use sim_core::{solve_ac, AcExcitation, AcOptions, AcSweep};

let options = AcOptions {
    sweep: AcSweep::Logarithmic { start: 10.0, stop: 1_000_000.0, points: 101 },
    excitations: vec![AcExcitation {
        component: input_source, magnitude: 1.0, phase_degrees: 0.0,
    }],
    ..AcOptions::default()
};
let result = solve_ac(&netlist, options)?;
```

| Type | Contenu |
| --- | --- |
| `AcOptions` | balayage, excitations, options DC |
| `AcSweep` | liste explicite, linéaire ou logarithmique |
| `AcExcitation` | source, amplitude crête, phase en degrés |
| `AcResult` | point DC + échantillons ordonnés |
| `AcSample` | fréquence, tensions, courants de branche |
| `Phasor` | composantes rectangulaires, magnitude, phase |
| `AcError` | balayage/excitation, point DC, singularité, valeur non finie |

Le DC est calculé avec `AcOptions::solver`. Les excitations ne modifient pas ce
point et `dc` n'est jamais réutilisé comme amplitude AC. Une source absente a
une excitation nulle : court-circuit petits signaux pour une source de tension,
circuit ouvert pour une source de courant.

Un balayage explicite exige des fréquences finies, strictement positives et
croissantes. Un balayage généré accepte `start <= stop` ; un seul point vaut
`start`, plusieurs exigent `start < stop` et conservent exactement les bornes.
Le linéaire interpole `start`/`stop` uniformément, le logarithmique interpole
`ln(start)`/`ln(stop)`. Le nombre de points est limité à `1_000_000`. Défaut :
logarithmique 1 Hz à 1 MHz, 101 points, sans excitation. Une analyse sans
excitation renvoie une réponse nulle.

Les phasors sont des amplitudes crêtes, sans RMS implicite. Les courants de
branche sont fournis pour sources de tension, inductances et sorties d'opamp ;
les autres se déduisent du modèle. La réponse AC ne refait pas de Newton : les
non-linéarités sont figées au point DC.

## 7. Erreurs

| Situation | Erreur |
| --- | --- |
| Netlist invalide | `DcError::Circuit(MnaError::Netlist(..))`, `TransientError::Circuit`, `AcError::OperatingPoint` |
| Composant sans modèle | `MnaError::UnsupportedComponent` |
| Paramètre absent/mauvais type/domaine | `MissingParameter`, `InvalidParameterType`, `InvalidParameterValue` |
| Saturation opamp incohérente | `MnaError::InvalidSaturation` |
| Branche dégénérée | `MnaError::DegenerateBranch` |
| Nœud flottant/conflit | `DcError::SingularMatrix` |
| Valeur non finie | `DcError::NonFiniteSolution` |
| Non-convergence | `DcError::NoConvergence` |
| Options invalides | `DcError::InvalidOptions` |
| Point initial/pas/événement/limite | `TransientError::InitialDc`, `Step`, `InvalidEvent`, `TooManySteps` |
| Balayage/excitation/point DC AC | `AcError::InvalidSweep`, `InvalidExcitation`, `OperatingPoint` |
| Matrice ou solution AC | `AcError::SingularMatrix`, `NonFiniteSystem`, `NonFiniteSolution` |

Un nœud relié seulement par un condensateur est résolu par le court-circuit
initial du transitoire ; le calcul DC classique le laisse flottant et échoue,
ce qui reste un diagnostic normal. Le worker renvoie
`AnalysisFailureKind::Dc` et l'interface conserve le dernier résultat valide.

## 8. Déterminisme, tests et limites

L'assemblage suit l'ordre des composants, alloue les branches dans cet ordre,
trie les événements par instant puis composant et génère les fréquences dans
l'ordre croissant. Aucune horloge, graine ou table de hachage n'intervient.

Les tests couvrent les circuits linéaires et non linéaires, les événements
alignés ou non, les stratégies de convergence, l'énergie, les balayages et les
erreurs ; [TESTING.md](TESTING.md) en donne la synthèse.

Limites : pas adaptatif, excitations continues non paramétrées, AC sans capacité
de jonction, opamp sans alimentation/slew rate/pôle, solveur dense.

## 9. Référence shunt programmable

`ComponentKind::ShuntReference` utilise les broches ordonnées `REF`, `K`, `A` et les cinq paramètres `vref`, `iref`, `ik_min`, `imax`, `z_ka`. DC et chaque pas transitoire résolvent la même loi algébrique bornée ; `I_KA` est positif de K vers A et `iref` va de REF vers A. La tension principale du composant est `V(K)-V(A)`. `branch_currents` ne contient pas ce courant non linéaire ; les tensions suffisent à le recalculer avec le modèle.

L'AC petits signaux inclut les dérivées réelles de la loi au point DC sur les broches K, A et REF. Le modèle est instantané : il n'ajoute aucun pôle, capacité, bruit ni comportement de démarrage. Voir [COMPONENTS.md](COMPONENTS.md) pour les valeurs et limites.

## 10. Comparateur à collecteur ouvert

`ComponentKind::Comparator` utilise l'ordre `in+`, `in-`, `out`, `vcc`, `vss`. La sortie est un sink référencé à `vss` ; il faut ajouter un pull-up externe pour obtenir l'état haut. Le modèle ne compare que si `2 V <= V(vcc)-V(vss) <= 36 V`. Le seuil bascule lorsque `V(in+)-V(in-)` passe sous `v_offset`. L'état commute sans délai ni hystérésis, au point DC comme à l'échantillon transitoire suivant.

L'AC conserve l'état logique du point DC et ne transfère pas l'excitation d'entrée à la sortie ; dans l'état sink, il linéarise `r_sink` tant que `imax` n'est pas atteint. Le courant non linéaire `out -> vss` est recalculé depuis les tensions, et n'est pas un courant de branche retourné par le solveur. Sans pull-up ni autre chemin dans l'état relâché, la matrice peut être singulière. Limites et défauts LM393 : [COMPONENTS.md](COMPONENTS.md).
