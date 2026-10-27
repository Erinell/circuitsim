# Exemples

Les exemples utilisent uniquement les API publiques et se lancent sans navigateur
ni ngspice.

## 1. Filtre RC : DC, transitoire et AC

```text
cargo run -p sim-core --example rc_response
```

Le circuit est un passe-bas `R1 = 1 kΩ`, `C1 = 1 µF`, `V1`. L'exemple calcule le
DC, l'AC à 1 Hz, à la coupure et à 1 MHz, puis 5 ms de transitoire avec échelon
de 5 V à `t = 0`.

```text
DC : V(OUT) = 0.000000 V
AC : f =        1.000 Hz, |V(OUT)| = 0.999980 V, phase = -0.360°
AC : f =      159.155 Hz, |V(OUT)| = 0.707107 V, phase = -45.000°
AC : f =  1000000.000 Hz, |V(OUT)| = 0.000159 V, phase = -89.991°
Transitoire : 501 échantillons, V(OUT, 5 ms) = 4.965463 V
```

Le DC est nul car le condensateur est ouvert ; à 5 ms, environ cinq constantes de
temps, la sortie atteint 4,965 V. Code :
`crates/sim-core/examples/rc_response.rs`.

## 2. Composant personnalisé NMOS

```text
cargo run --example custom_mosfet
```

L'exemple enregistre `Custom("nmos")` avec les broches `d`, `g`, `s`, le place
dans un schéma, l'extrait, sérialise le JSON et vérifie le refus du solveur.

```text
Registre : 12 symboles, dont 1 personnalisé(s).
Netlist : 2 composant(s), M1 = Custom("nmos").
JSON : 1877 octets, kind personnalisé restauré = true.
Simulation : refus explicite MnaError::UnsupportedComponent pour « nmos ».
Pour simuler ce transistor, ajouter un modèle dans sim-core puis accepter ce kind dans MNA.
```

Les trois broches sont reliées à la masse pour isoler le contrat de
représentation du modèle électrique.

Pour ajouter un vrai modèle : définir le symbole et ses paramètres dans
`editor-core` ; vérifier l'extraction `ComponentKind::Custom` ; ajouter les
équations DC/transitoire/AC et les dérivées dans `sim-core` ; accepter le type
dans MNA ; ajouter les tests des trois analyses ; exposer le symbole dans l'UI
seulement ensuite. Tant que le modèle manque, `UnsupportedComponent` est le
comportement contractuel.

## 3. Application et Web

```text
cargo run -p circuitsim-app
cargo xtask wasm-check
cargo xtask web-build
cargo xtask web-serve
```

L'application ouvre le circuit RC de démonstration et le panneau Instruments.
La galerie **Fichier → Exemples** contient RC, diviseur, LED, interrupteur, RLC
série et RLC parallèle commuté.
`web-build` produit `dist/`, `web-serve` écoute sur `http://127.0.0.1:8080`.
Utilisation : [UI.md](UI.md), [INSTRUMENTS.md](INSTRUMENTS.md) ; déploiement :
[DELIVERY.md](DELIVERY.md).

## 4. Benchmarks

```text
cargo bench -p sim-core --bench solvers
cargo bench -p editor-core --bench extraction
cargo bench -p circuitsim-app --bench instruments
```

Ils couvrent DC, transitoire RC 501 échantillons, AC 101 points, extraction de
16/64/256 résistances, FFT 16 384 points et CSV. Pour un ordre de grandeur :

```text
cargo bench -p sim-core --bench solvers -- --quick --noplot
```

Charges, résultats et budgets : [PERFORMANCE.md](PERFORMANCE.md).
