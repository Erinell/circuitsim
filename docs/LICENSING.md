# Licence et dépendances

## 1. Choix du projet

CircuitSim est distribué sous **`MIT OR Apache-2.0`**. L’utilisateur choisit
l’une des deux licences. Apache-2.0 apporte une concession explicite de brevets ;
MIT reste courte et largement compatible avec l’écosystème Rust.

- Textes intégraux : [LICENSE-MIT](../LICENSE-MIT) et
  [LICENSE-APACHE](../LICENSE-APACHE).
- Titulaire : **Cédric Debois**, année **2026**.
- Métadonnées : `license = "MIT OR Apache-2.0"` dans `[workspace.package]`,
  hérité par chaque crate avec `license.workspace = true`.
- Toute contribution est soumise aux mêmes termes.
- Aucun en-tête de licence n’est ajouté dans les fichiers source ; la licence est
  portée par les fichiers racine et les métadonnées SPDX.

## 2. Politique de dépendances

`cargo-deny` vérifie le graphe avec `deny.toml`. La liste autorise les licences
permissives nécessaires : MIT, Apache-2.0, Apache-2.0 avec exception LLVM,
BSD-2/3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, 0BSD, BSL-1.0, CC0-1.0,
Unlicense, OFL-1.1 et Ubuntu-font-1.0.

Les licences copyleft fortes (`GPL`, `AGPL`, `LGPL`) ne sont pas autorisées pour
le code distribué ; lorsqu’une crate propose une alternative MIT/Apache, cette
alternative est retenue. Les exceptions réellement présentes dans le graphe et
leur justification sont consignées dans [DECISIONS.md](DECISIONS.md).

Vérification locale et CI :

```text
cargo deny check licenses
```

Une nouvelle dépendance doit être ajoutée avec sa licence vérifiée, puis
documentée si elle nécessite une exception ou une décision de compatibilité.

## 3. Licences tierces distribuées

`THIRD_PARTY_LICENSES.md` liste les licences des crates embarquées dans le
binaire WASM distribué. Il est généré, jamais édité à la main :

```text
cargo about generate about.hbs --workspace \
  --target wasm32-unknown-unknown \
  --output-file THIRD_PARTY_LICENSES.md
```

`about.toml` fixe la cible WASM et les licences acceptées ; `about.hbs` est le
modèle Markdown. La CI vérifie les licences avec `cargo-deny`. Le fichier tiers
est régénéré à chaque release et publié avec la distribution.

## 4. Assets et exemples

Les images, polices, sons, schémas et exemples ajoutés doivent être créés par le
projet ou couverts par une licence compatible avec `MIT OR Apache-2.0`. Toute
ressource tierce doit indiquer sa source, son auteur et sa licence, et être
ajoutée aux licences tierces si elle est distribuée.

Ne pas copier de symbole, de police, de schéma ou de texte sans licence
explicite. Un asset dont la licence est inconnue est refusé.

## 5. Checklist release

- `cargo deny check licenses` vert ;
- `THIRD_PARTY_LICENSES.md` régénéré et versionné ;
- `README.md` et `docs/DECISIONS.md` à jour ;
- absence d’en-têtes de licence dans les sources ;
- `Cargo.lock` versionné ;
- artefacts natifs et WASM accompagnés des licences du projet et des tiers.
