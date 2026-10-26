# Protocole worker et pont WASM

Le protocole transporte une netlist et des options validées entre l'interface et
le worker, puis renvoie un résultat complet ou une erreur typée. Il ne dépend ni
du dessin du schéma ni d'une horloge globale. Les crates concernées sont
`worker-protocol` et `worker`.

L'application native et l'hôte web utilisent `WorkerService` dans leur fil ; le
worker dédié du navigateur et `SharedArrayBuffer` restent à brancher. Les tests
du protocole et du worker ne demandent ni navigateur ni SAB.

## 1. Trame

```text
offset  taille  contenu
0       4       magie ASCII "CSIM"
4       2       version u16 little-endian
6       1       direction : 1 = requête, 2 = réponse
7       1       drapeaux réservés, toujours 0
8       4       longueur de charge utile, u32 little-endian
12      N       charge utile postcard
```

`HEADER_LEN = 12`. Le décodeur vérifie dans l'ordre : en-tête complet, magie,
version, direction attendue, drapeaux, longueur exacte, puis payload postcard.
Une trame tronquée, des octets excédentaires, une version/direction inconnue ou
des drapeaux non nuls produisent une `ProtocolError`. Un décodeur de requête
refuse une réponse et inversement.

### Netlist sur fil

La netlist JSON utilise un `ParamValue` Serde à tag interne, incompatible avec
postcard. Le protocole a donc une représentation dédiée : nœuds (id, nom, type
`Ground`/`Electrical`/`Internal`), composants (id, kind, label, paramètres,
broches) et paramètres en liste ordonnée `Number`/`Bool`/`Text`. La
désérialisation refuse nom invalide, doublon, paramètre inconnu ou netlist
invalide, puis reconstruit avec `Netlist::new`. Le JSON de schéma et
`SCHEMA_VERSION = 1` sont indépendants.

## 2. Messages

```text
ClientMessage
├── Submit { job: { revision: u64, netlist, analysis } }
└── Ping { nonce: u64 }

ServerMessage
├── Accepted { revision }
├── Coalesced { revision, replaced }
├── Rejected { revision, reason }
├── Result { revision, result }
├── Failed { revision, failure }
└── Pong { nonce }
```

`Analysis` sélectionne `Dc(DcOptions)`, `Transient(TransientOptions)` ou
`Ac(AcOptions)`, avec les résultats et erreurs publics de `sim-core`. Une netlist
reçue est toujours revalidée pour la simulation. `Accepted` et `Coalesced` sont
immédiats ; `Result`/`Failed` arrivent via `poll`, au plus une réponse par job.
`Ping` renvoie immédiatement `Pong` sans lancer d'analyse.

`RejectionReason` : `InvalidRevision` (révision zéro réservée) ou
`StaleRevision { latest }` (révision non strictement supérieure). `AnalysisFailure`
porte une étape `Netlist`, `Dc`, `Transient` ou `Ac` et un message stable
d'affichage, jamais un découpage programmatique.

## 3. Révisions

- révision strictement positive ;
- révisions acceptées strictement croissantes ;
- une révision ≤ dernière acceptée est refusée sans remplacer le job ;
- la dernière révision acceptée persiste après exécution ;
- accusés et résultats répètent la révision.

Un nouveau worker repart d'une nouvelle session ; aucune révision globale entre
processus n'est supposée.

## 4. Ordonnancement

Un seul emplacement en attente :

1. vide → `Accepted` ;
2. occupé → remplacement de l'ancien, `Coalesced` ;
3. révision ancienne/nulle → `Rejected`.

Cadence par défaut 60 Hz : après un démarrage à `t`, pas de nouvelle analyse
avant `t + 1/60 s`, sans rattrapage en rafale. `WorkerConfig` permet d'ajuster la
cadence ; le pont WASM utilise 60 Hz.

Le worker ne lit aucune horloge. `poll(now_seconds)` reçoit un temps fini et
monotone ; un temps invalide produit `WorkerError::InvalidTime` ou
`TimeWentBackwards`. La coalescence porte sur les démarrages : une analyse longue
occupe le worker, les jobs reçus entre-temps ne s'exécutent pas en parallèle.

## 5. Pont WASM

`CircuitWorker` expose :

| Export | Effet |
| --- | --- |
| `new CircuitWorker()` | worker à 60 Hz |
| `submitFrame(frame)` | traite une requête, renvoie l'accusé ou le pong |
| `pollFrame(nowSeconds)` | exécute au plus un job, renvoie une réponse ou `undefined` |
| `hasPending()` | vrai si un job attend son créneau |
| `protocolVersion()` | version du protocole |

Les trames sont des `Uint8Array`. Une erreur de protocole ou de temps devient une
exception JavaScript à capturer. Le worker dédié doit créer `CircuitWorker` dans
son propre contexte, recevoir les trames par `message`, appeler `pollFrame` avec
`performance.now()/1000`, renvoyer les réponses avec transfert des buffers et
planifier un prochain appel si `hasPending()`. Le client natif actuel n'utilise
pas ce pont : il appelle `WorkerService` directement.

## 6. Transports

| Mode | Condition | Usage |
| --- | --- | --- |
| `PostMessage` | toujours | référence, octets copiés |
| `SharedArrayBuffer` | SAB + page cross-origin isolée | canal partagé, mêmes trames |

`select_transport(shared_available, cross_origin_isolated)` ne choisit SAB que si
les deux conditions sont vraies, sinon `PostMessage`. Le protocole ne change pas
selon le transport. L'application actuelle utilise le service en fil ; le
branchement SAB/worker dédié reste à faire.

## 7. Bornes, versionnage et tests

- Payload limité par `u32` ; dépassement refusé à l'encodage
  (`PayloadTooLarge`).
- Aucune confiance dans la version, direction, longueur ou contenu Serde.
- Un échec produit `Failed`, jamais un `Result` partiel.
- Aucun historique de résultats ni file de jobs ; seuls le job en attente et la
  dernière révision persistent.
- Une réponse volumineuse reste une trame complète ; pas de streaming.
- `PROTOCOL_VERSION = 1`. Toute modification incompatible de l'en-tête, des
  messages/champs, de la netlist sur fil, des types Serde transportés ou de la
  sémantique de révision exige une nouvelle version.
- Le décodeur refuse une autre version avant de lire le payload.

Les 11 tests protocole et 13 tests worker couvrent en-tête, frames malformées,
netlist, messages, révisions, coalescence, cadence, horloge, analyses et
transports. Voir [TESTING.md](TESTING.md).
