# Instruments, mesures et export

Le panneau egui transforme les résultats typés du worker en traces, mesures,
spectre, Bode et CSV. Il ne duplique ni la connectivité ni les solveurs. Les
contrôles et raccourcis sont dans [UI.md](UI.md).

## 1. Exécution

`SimulationClient` possède un `WorkerService` et un compteur de révision propre à
l'interface, indépendant de la révision du document.

1. Les champs d'ingénierie sont parsés et validés, limites comprises.
2. La netlist extraite est clonée dans un `Job` avec une révision croissante.
3. `encode_request` produit une trame ; `submit_frame` renvoie l'accusé.
4. `poll_frame` exécute au plus une analyse ; le résultat est associé aux tables
   de noms et à la révision capturées avant l'envoi.
5. Traces et mesures sont construites avec les `NodeId`/`ComponentId`, jamais des
   indices devinés.

Un seul job attend. Une nouvelle soumission ne remplace pas silencieusement un
job en cours. Le service tourne dans le fil de l'interface ; worker dédié et
`SharedArrayBuffer` restent à brancher. Un résultat reste consultable après une
modification du schéma, avec un avertissement s'il est antérieur.

Le transitoire peut être lancé depuis le panneau, par **Marche/Arrêt** dans la
barre d’outils, `Espace`, `F5` ou `Ctrl+Entrée`. À l’ouverture d’un schéma
valide, il démarre automatiquement. La portée utilise une **session temps
réel** : aucun arrêt n'est demandé, le pas fixe est conservé, la vitesse de
lecture et la pause sont réglables. Les modifications de paramètres sont
appliquées à chaud tant que la topologie reste compatible ; une modification de
topologie relance la session en conservant pause et vitesse. Le dernier
échantillon alimente l'inspecteur, les sondes et la superposition des courants :
des points jaunes à halo sombre parcourent chaque net dans le sens des injections
positives, et les LED rayonnent selon leur tension directe.

## 2. Catalogue et sélection

`SignalCatalog::from_extraction` produit trois listes triées :

- tensions des nets, masse exclue ;
- courants de branche disponibles (sources de tension, inductances, opamps) ;
- sources indépendantes avec valeur `dc` en SI.

Les valeurs sont formatées à quatre chiffres significatifs. `OUT` est prioritaire
pour le spectre et le Bode ; sinon le premier net alphabétique. La sélection de
portée est un `BTreeSet`, donc l'ordre des courbes et lignes CSV est stable.

## 3. Transitoire et échelon

Les champs de temps acceptent `5m`, `10u`, etc. Seul le **pas de calcul** reste
configurable dans le mode temps réel ; la session n’a pas de borne d’arrêt. La
vitesse multiplie le temps simulé par seconde réelle, et la pause fige la
fenêtre glissante sans perdre les échantillons. Le rapport temps simulé/réel est
affiché ; sous 65 % de la cible, le badge `trop lent` signale que le moteur ne
suit pas. La luminosité de puissance et la vitesse d’animation des courants sont
réglables indépendamment.

Une modification de paramètre est appliquée à la session par
`TransientSession::update_netlist` : la topologie, les noms de nets et les
références doivent rester identiques. Le temps, la solution précédente et les
énergies déjà accumulées sont conservés. Toute autre modification relance la
session.

L'échelon de source :

1. remplace `dc` par la valeur initiale dans le clone ;
2. ajoute un `TransientEvent` avec la valeur finale ;
3. exige un instant dans `[début, arrêt)` ;
4. laisse le worker résoudre l'échantillon initial puis appliquer l'événement
   après l'échantillon exact.

La source du document n'est jamais modifiée. L'option fonctionne pour une source
de tension ou de courant ; les autres formes d'onde ne sont pas paramétrées.

Le dernier échantillon est exposé comme `LiveValues` : tensions par nom de net,
courants de branche par référence, révision du document et instant. L’inspecteur
en dérive les tensions de broches, le courant et la puissance. Les sondes
placées sur le schéma alimentent la liste **Sondes** et les bulles du canevas :
voltmètre sur un fil, ampèremètre et ohmmètre sur un composant.

## 4. Portée et mesures

Chaque signal devient une `Trace` de points `[temps_s, valeur_SI]` en ordre
croissant. En temps réel, la portée ne conserve que les 500 derniers
échantillons pour l’affichage, alors que le tampon complet alimente le spectre.
Le rendu décime à 4 096 points en conservant les extrémités ; mesures et export
utilisent les valeurs disponibles. Les couleurs suivent un huit-uplet fixe.

Mesures sur les échantillons, sans pondération temporelle supplémentaire :

```text
min   = min(x_i)
max   = max(x_i)
crête = max - min
moy   = somme(x_i) / N
RMS   = sqrt(somme(x_i^2) / N)
```

Un pas fixe rend la moyenne pertinente. Si un événement ajoute un échantillon
exact, elle reste diagnostique et non une intégrale temporelle.

Deux curseurs A/B alignés sur les échantillons donnent tA, tB, Δt, 1/Δt, les
valeurs en A/B, leur écart, et min/max/RMS sur l’intervalle. Les calculs sont
refaits à chaque image à partir des traces complètes ; aucun résultat du worker
n’est modifié.

## 5. Spectre FFT

Calcul dans `crates/app/src/analysis.rs` avec `rustfft` en `f64`, sans SIMD pour
rester portable en WASM.

1. Durée et pas nominal donnent le nombre de pas disponibles.
2. Taille FFT = plus petite puissance de deux ≥ ce nombre, bornée à 16–16 384.
3. Rééchantillonnage linéaire depuis le premier échantillon, sans dupliquer le
   dernier.
4. Retrait de la moyenne puis fenêtre de Hann périodique.
5. Spectre unilatéral normalisé par la somme de la fenêtre, facteur 2 sauf DC et
   Nyquist.
6. Résolution `1 / durée` ; pic = plus grande composante non DC.

Un sinus de 2 V sur un bin donne 2 V à 1 % près. Un signal non périodique ou un
événement dans la fenêtre produit des fuites spectrales : Hann les réduit sans
les annuler. Le spectre est une observation, pas une mesure certifiée.

## 6. Bode

L'analyse AC est un calcul petits signaux autour du DC. La sortie est une tension
choisie parmi les nets.

```text
gain_dB(f) = 20 * log10(|V_out(f)| / A_excitation)
phase(f)   = phase(V_out(f))
```

`A_excitation` est l'amplitude crête d'`AcExcitation` ; la phase source vaut zéro.
Pour une source de courant, le gain est une transimpédance V/A bien que l'axe
reste en dB. Une magnitude nulle est omise. L'axe x stocke `log10(f)` et les
graduations affichent Hz/kHz/MHz. Gain et phase partagent l'axe ; la phase est
affichée entre -180° et +180°. Défaut RC : 1 Hz à 1 MHz, 101 points.

Un clic sur l’un des deux graphes pose un marqueur de fréquence aligné sur le
point AC le plus proche. Le marqueur est commun au gain et à la phase ; le
panneau affiche fréquence, gain et phase. Il ne modifie ni le balayage ni le
résultat.

## 7. Export CSV

Point décimal, nombres en notation scientifique à 12 décimales, champs échappés
selon RFC 4180. L'export contient toutes les valeurs de l'onglet actif, sans
décimation.

```text
Portée  : signal,unit,time_s,value
Spectre : signal,unit,frequency_hz,magnitude
Bode    : signal,frequency_hz,magnitude_v,gain_db,phase_deg
```

La portée est en format long, une ligne par signal et échantillon. Le spectre
inclut DC et Nyquist. Le Bode n'exporte que les magnitudes strictement positives.

Sur poste de travail, un chemin vide utilise le nom proposé dans le répertoire
courant. Sur le Web, un `Blob` UTF-8 est téléchargé par un lien temporaire. Noms :
`circuitsim-portee.csv`, `circuitsim-spectre.csv`, `circuitsim-bode.csv`.
L'export ne sauvegarde pas le schéma.

## 8. Limites et tests

Limites : worker et session temps réel dans le fil de l'interface, transitoire
à pas fixe, FFT limitée et sans fenêtres alternatives, pas de moyenne
glissante/analyse harmonique, CSV seulement (pas PNG/SVG ni session), 100 000
échantillons temps réel en mémoire et 10 000 points AC, résultats en mémoire
avec avertissement s'ils deviennent antérieurs. La session peut être mise en
pause mais pas reprise depuis un état sauvegardé.

Les tests couvrent catalogue, échelon, limites, transitoire et AC via worker,
mesures, FFT étiquetée, décimation, trois CSV, échappement, export natif UTF-8 et
rendu headless. Voir [TESTING.md](TESTING.md).
