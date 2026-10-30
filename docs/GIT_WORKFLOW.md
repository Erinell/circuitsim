# Workflow Git

## 1. Règles de base

- `main` est toujours vert : format, clippy, tests, build WASM et licences.
- Aucun commit de travail direct sur `main`, sauf l’import initial et les fusions.
- Une branche par lot cohérent : `feat/<sujet>`, `fix/<sujet>`,
  `refactor/<sujet>`, `docs/<sujet>`, `chore/<sujet>`.
- Fusion dans `main` avec `git merge --no-ff <branche>`, puis suppression de la
  branche.
- Pas de `git push --force`, pas de réécriture de l’historique de `main`, pas de
  `git reset --hard` sans accord explicite.
- Aucun remote ni push ne sont configurés sans validation de l’URL.

## 2. Commits

Format Conventional Commits :

```text
<type>(<portée>): <description à l'impératif, ≤ 72 caractères>
```

Types autorisés : `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`,
`ci`, `chore`. Portées : `editor`, `ui`, `scope`, `engine`, `solver`,
`components`, `protocol`, `worker`, `docs`, `ci`, `license`, `git`.

Un commit contient un changement logique, compilable et testé. Le corps explique
le *pourquoi* si nécessaire. Exemples :

```text
feat(editor): insère un composant en série sur un fil
fix(scope): conserve le zoom après un changement de canal
docs(git): documente les fusions sans fast-forward
```

Avant chaque commit :

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check licenses
```

Stager uniquement le sujet avec `git add -p` ou des chemins explicites.

## 3. Branches et fusions

```text
git switch main
git switch -c feat/command-palette
# commits et vérifications
git switch main
git merge --no-ff feat/command-palette
git branch -d feat/command-palette
```

Une branche brouillonne peut être intégrée en squash après validation. Une
branche non fusionnée peut être supprimée avec `git branch -D` après accord.

## 4. Retours arrière sûrs

- Annuler un commit public : `git revert <commit>` (crée un commit inverse).
- Restaurer un fichier depuis `HEAD` : `git restore <fichier>`.
- Récupérer un fichier d’un commit : `git restore --source=<commit> <fichier>`.
- Mettre de côté un travail : `git stash push -m "sujet"`.
- Corriger le dernier commit local non poussé : `git commit --amend`, uniquement
  avant partage.
- Éviter `reset --hard` ; préférer une branche de sauvegarde ou `revert`.

## 5. Tags et versions

- Tags annotés SemVer : `git tag -a v0.1.0 -m "CircuitSim v0.1.0"`.
- `v0.1.0` marque l’import initial.
- `v0.2.0` marque la mise à jour ergonomie et gouvernance.
- Le `CHANGELOG.md` est mis à jour dans le même lot qu’un `feat` ou `fix`.

## 6. Réglages GitHub recommandés

À configurer dans l’interface GitHub, sans automatisation depuis ce dépôt :

- protéger `main` : pull request obligatoire, au moins une revue, checks requis ;
- exiger `Format, lint, tests, WASM and benches` et le job de licences ;
- interdire les pushes directs et les force-pushes sur `main` ;
- supprimer automatiquement les branches après fusion ;
- activer les alertes Dependabot et les mises à jour de sécurité ;
- protéger les tags de release ;
- limiter les permissions du `GITHUB_TOKEN` au strict nécessaire.

## 7. Release

1. Vérifier `cargo xtask check`, `wasm-check`, `web-build`, `cargo deny check
   licenses` et les benchmarks.
2. Régénérer `THIRD_PARTY_LICENSES.md`.
3. Mettre à jour `CHANGELOG.md` et la version du workspace.
4. Fusionner la branche de release et vérifier `main`.
5. Créer le tag annoté, sans push automatique.
6. Déclencher `.github/workflows/release.yml` avec le tag : le workflow construit
   Windows, Linux et le web, puis publie la GitHub Release avec les archives et
   les sommes SHA-256.
7. Vérifier la page de release et les artefacts publiés.
