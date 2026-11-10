# Audit complet de SimpleEdit 1.25.0

> Commit `3f7f4bb` (branche `main`) · audit du 9 novembre 2026
> Périmètre : ~115 000 lignes de Rust (`src/`, `drivers/`, `tests/`), plus `install.sh`, `uninstall.sh`, la CI GitHub, `Cargo.toml`, les patches `vendor/`, `debian/` et `snap/`.

## Sommaire

1. [Méthode](#1-méthode)
2. [Verdict général](#2-verdict-général)
3. [Priorités](#3-priorités)
4. [Problèmes transverses](#4-problèmes-transverses)
5. [Fonctionnalités manquantes importantes](#5-fonctionnalités-manquantes-importantes)
6. [Plan de correction proposé](#6-plan-de-correction-proposé)
7. Annexes A à H : rapports détaillés par domaine

---

## 1. Méthode

**Mesures automatiques**
- `cargo clippy --all-targets` : **0 avertissement**.
- `cargo test` : **1 122 tests passés, 0 échec, 19 ignorés**. Les tests ignorés sont les tests « live » (PostgreSQL réel, rust-analyzer, installation de serveurs) et les captures d'écran.
- Les clés de traduction EN et FR sont identiques, ce que vérifie déjà `tests/unit_i18n.rs`.

**Lecture du code.** Le code a été découpé en 8 domaines, et chaque fichier a été lu en entier, avec ses appelants (Annexes A à H). Chaque constat donne :
- l'emplacement `fichier:ligne` ;
- une sévérité ;
- un scénario concret (entrée ou action, puis mauvais résultat) ;
- un correctif en une ou deux phrases.

Un constat qui n'a pas pu être confirmé jusqu'au bout est marqué **[plausible]**.

**Contre-vérification.** Les constats les plus graves ont été revérifiés indépendamment :

| Marque | Signification |
|---|---|
| ✅ **test** | Reproduit par un test unitaire temporaire. Ces tests échouaient sur `3f7f4bb` ; ils sont maintenant dans le dépôt et passent. |
| ✅ **repro** | Reproduit dans un dépôt git jetable (git 2.53). |
| 🔎 **code** | Chemin de code relu et confirmé une seconde fois. |
| *(rien)* | Constat de l'audit du domaine, vérifié par lecture du code. |

**Sévérités**

| Niveau | Définition |
|---|---|
| **critique** | Perte ou corruption de données dans un usage courant. |
| **haute** | Perte de données dans un cas réaliste, faille exploitable, ou fonction cassée. |
| **moyenne** | Comportement faux ou coûteux qu'un utilisateur rencontrera. |
| **basse** | Cas limite, confort, finition. |

**Limites**
- L'interface n'a pas été lancée à la main.
- Les tests live (PostgreSQL, MySQL, serveurs de langage) n'ont pas été exécutés.
- macOS n'a pas été testé.
- Les numéros de ligne renvoient au commit `3f7f4bb`.

---

## 2. Verdict général

**Le code est de bonne qualité.** Clippy est propre, plus de mille tests passent, l'i18n est complète en anglais et en français, et la sécurité de la chaîne de distribution est sérieuse.

**Points solides, vérifiés**
- **Mise à jour automatique :**
  - signature Ed25519 vérifiée avec `verify_strict` ;
  - nom d'asset lié à la version, ce qui empêche un retour à une ancienne version signée ;
  - dossier en 0700, et nouveau hachage par root sous Linux ;
  - les pilotes de base de données passent par la même vérification.
- **Installation et CI :**
  - `install.sh` refuse d'installer sans vérification, sauf avec `--insecure` ;
  - les actions GitHub sont épinglées par SHA, sans injection `${{ }}` dans les `run:` ;
  - les déclencheurs sont `pull_request`, pas `pull_request_target`.
- **Secrets :**
  - `session.json` et les fichiers de l'API sont en 0600 ;
  - les mots de passe des bases passent par stdin ou par l'environnement, jamais par les arguments de processus ;
  - le trousseau système est utilisé.
- **Base de données et Git :**
  - PostgreSQL `verify-full` vérifie bien le nom d'hôte, y compris à travers le tunnel (`hostaddr`) ;
  - pas d'interblocage de tubes avec les pilotes ;
  - Git est protégé contre l'injection d'arguments (`--`, `--end-of-options`, `verify_rev`).
- **Aperçus :**
  - le Markdown ne charge aucune ressource distante et affiche le HTML comme du texte ;
  - le JSON a une limite de profondeur.

**Faiblesses.** L'audit relevait **environ 230 constats** et une cinquantaine de fonctionnalités manquantes. Les 15 priorités P0 (pertes de données silencieuses, failles) puis 149 autres constats prioritaires ont été corrigés et retirés de ce fichier. Il reste **environ 42 constats**. Les fragilités restantes sont **systémiques** et reviennent dans tous les domaines (section 4) :
- quelques réponses asynchrones sans identifiant de requête (le gros est traité) ;
- une partie Base de données pensée pour PostgreSQL puis étendue à MySQL et SQLite (le dialecte est maintenant partout, sauf le designer qui reste PostgreSQL) ;
- beaucoup de travail synchrone ou périodique sur le thread de l'interface ;
- un cycle de vie des processus externes (LSP, tâches, SSH) incomplet.

| Annexe | Domaine | Constats (≈) | Critiques | Hautes |
|---|---|---:|---:|---:|
| A | Cœur de l'éditeur (widget, multi-curseurs, annulation, recherche, complétion, orthographe) | 2 | 0 | 1 |
| B | Application : cycle de vie des fichiers, session, onglets, sauvegarde auto, grands fichiers | 4 | 0 | 0 |
| C | Git (statut, diff, indexation, log, conflits, push) | 4 | 0 | 0 |
| D | Base de données, back-end (pilotes, protocole, tunnel SSH, TLS, trousseau) | 8 | 0 | 0 |
| E | Base de données, interface (grille, console, designer, complétion SQL) | 4 | 0 | 0 |
| F | Outils externes (LSP, lint, terminal, tâches, formateur, SSH) | 2 | 0 | 0 |
| G | Recherche dans les fichiers, Search Everywhere, projets, barre latérale | 7 | 0 | 2 |
| H | Sécurité, mise à jour, CI, dépendances, client API, aperçus, préférences | 11 | 0 | 0 |
| | **Total** | **≈ 42** | **0** | **≈ 3** |

**Observations complémentaires de l'auditeur**
- **`target/` occupe 40 Go**, dont 24 Go d'incrémental. Cela vient du profil dev (`opt-level = 3` sur toutes les dépendances) et de l'accumulation des builds. Un `cargo clean` de temps en temps, ou `debug = "line-tables-only"` pour les dépendances, réduit fortement cette taille.
- **`src/app.rs` fait 8 869 lignes** et `src/app/tests.rs` 5 229 lignes. Une seule structure `SimpleEditApp` porte tout l'état. Les bugs « réponse périmée » et « garde de fermeture incomplète » viennent en partie de là : chaque type d'onglet (texte, table, API, diff) gère son état « modifié » à sa façon.
- **La CI n'exécute jamais les tests des pilotes contre de vrais serveurs.** Un job avec des conteneurs de service PostgreSQL et MySQL aurait détecté des bugs de l'annexe D comme `CLIENT_FOUND_ROWS` (D · 12) ou l'enveloppe `SELECT * FROM (…)` (D · 14).
- **Les tests ne couvrent toujours pas les combinaisons multi-curseurs + commande** (déplacement de ligne, changement de casse). L'annulation, les sélections qui se chevauchent et les gardes d'écriture du mode grand fichier ont maintenant leurs tests.

---

## 3. Priorités

Références : `A · Bugs 5` = Annexe A, section Bugs, point 5. `D · 11` = Annexe D, point 11 (numérotation continue). `G · P2` / `H · S1` = identifiants propres à l'annexe. Les points corrigés ont été retirés (la numérotation garde ses trous).

Les 15 priorités P0 de l'audit initial sont corrigées, ainsi que 10 priorités P1 (fichiers binaires, `git init` dans `$HOME`, `SET` PostgreSQL, SELECT différés, connexion de la console, LSP, panneau Résultats, commandes de ligne et casse multi-curseurs, signature des releases, correcteur orthographique), puis 134 autres (session MySQL morte, Échap, export Search Everywhere, cellules de grille, `rust-analyzer --version`, regroupement des suppressions dans l'annulation, « Enregistrer sous », « Recharger avec l'encodage », conflits indexés, `--force-if-includes`, `branch -d`, fusion 3 voies, `CLIENT_FOUND_ROWS`, COMMIT SQLite, Stop MySQL, filtre psql, raccourcis de la grille, sélecteur DATETIME, arrêt des tâches, Couper multi-curseurs, indexation CRLF et sans fin de ligne, préfixes de diff, diff de merge, `busy` Git, `didSave`, `shutdown` LSP, URI, grands fichiers qui grossissent, Ctrl+D par mots, bloc en colonnes visibles, clés BINARY MySQL, comparaisons SQLite, export temporaire privé, diagramme et copie de table multi-bases, Ctrl+C de la grille, `$1_x`, renommage de dossier, config d une version plus récente, `publishDiagnostics`, guillemets des tâches, `curl -d`, options curl, requêtes Postman, installation en root, socket IPC, relance après mise à jour, chemins non UTF-8 de l IPC, Search Everywhere sur les buffers non enregistrés, `~` du champ dossier, renommage de casse, envoi SSH atomique, copies locales distinctes, `rustfmt` selon le projet, liens `fichier:ligne` avec espaces, `ssh://hôte:port`, marques d espaces, dossier scratch, fichier supprimé, config d une fenêtre secondaire, blame sur le buffer, remote par défaut, renommages et drop dans Git, import JSON, designer, terminal décalé, lecture LSP bloquante, redessin continu du terminal, Échap et environnements de l API, listes par id, aperçu Markdown, avertissement « déjà poussé » des réécritures, dépôt sans commit, conflits de stash, session écrite hors du clic de fermeture, reconnexion PostgreSQL en transaction, options courtes de curl, scripts Postman signalés, hash de mise à jour macOS, clé de l aperçu, messages de l API traduits, curseurs multiples en temps linéaire, onglet de résultat réutilisé, `run::fix` en une connexion, SSH en une connexion par opération, vue partagée sans copie inutile, « Tout enregistrer », re-coloration des diffs Git, arbre des bases regroupé, version des pilotes téléchargés, diagramme en cache, réception des réponses API hors du thread UI, Ctrl+C de la ligne, remplacement d une correspondance vide, recharger depuis le disque, sous-modules dans la recherche, fichiers ignorés comptés, variables dynamiques Postman, nombre total de lignes, collage TSV, PATH du shell de connexion, paquets de lignes des tâches).

### P1 : haute priorité

| Problème | Réf. | Vérif. |
|---|---|---|
| **Performance :** copie complète du buffer par point d'annulation (jusqu'à 64 Mo) ; les Retour arrière / Suppr consécutifs sont maintenant regroupés, mais Entrée, coller et chaque début de mot en font encore une. | A · Performance 2 | |

Toutes les sévérités moyennes et basses sont dans les annexes.

---

## 4. Problèmes transverses

Ces motifs reviennent dans plusieurs domaines. Les corriger une fois, de façon centrale, règle des dizaines de constats d'un coup.

### T1. Réponses asynchrones sans identifiant de requête
**Traité :** lecture d'onglet, pages et tri de la grille, `git status` / stashes / refs, log git, dialogue Push, Search Everywhere, écriture de la session, recherche dans les fichiers (annulation + pause avant le calcul), réponses du designer, de la santé et de l'import.

Reste à auditer au cas par cas les réponses des autres fenêtres (admin de tables, sauvegarde/restauration) : le même motif `DbMsg::Answer(fenêtre, message)` s'applique.

### T2. Gardes de fermeture et de destruction incomplètes
- Les suppressions de l'arbre sont définitives (pas de corbeille).
- Changer le dossier « scratch » déplace tous les fichiers d'un dossier existant.

**Correctif structurel :**
- un trait `has_unsaved_changes()` implémenté par chaque type d'onglet (`is_path_dirty` couvre maintenant texte, tables et requêtes API, mais par cas ; les onglets API modifiés sont aussi restaurés par la session) ;
- une confirmation pour toute action destructive restante ;
- le crate `trash` pour les suppressions.

### T3. Écritures disque non sûres
- « Enregistrer sous » peut écrire un fichier vide si l'onglet a été fermé pendant le dialogue.
- Les encodages et CRLF sont perdus dans plusieurs chemins : l'annulation dans un diff réécrit en UTF-8, le finder ignore les encodages détectés par l'éditeur.

**Correctif structurel :** une seule fonction d'écriture, `fsutil::atomic_write(path, texte, format, empreinte_attendue)`, qui :
1. refuse si le fichier a changé depuis la lecture ;
2. conserve l'encodage et les fins de ligne ;
3. enregistre l'historique local ;
4. refuse les chemins en mode grand fichier.

### T4. SQL non paramétré par dialecte
**Traité :** recherche et filtres de la grille, liens de clés étrangères, quotage de la complétion, « Copier en INSERT », export SQL, `EXPLAIN` (`EXPLAIN ANALYZE` pour MySQL, `EXPLAIN QUERY PLAN` pour SQLite), découpeur de la console (`\'`, `#`, backticks, `DELIMITER`, `BEGIN…END`), gardes `reads_only` / `risk_of` / `wrappable`, bouton d'exécution de la console (pilote du dialecte choisi). Le designer refuse les sources non PostgreSQL au lieu d'écrire du DDL faux.

**Reste :** un vrai designer pour MySQL et SQLite (voir section 5).

### T5. Travail périodique et synchrone sur le thread de l'interface

**Scrutations permanentes**
- Tick d'une seconde : `stat` des dossiers dépliés.
- `git status --ignored -uall` toutes les 3 s, même panneau fermé.
- `git show HEAD:file` toutes les 10 s.
- Panneau Git : rafraîchi toutes les 2 s, ce qui relance aussi la coloration syntaxique.
- Terminal : redessiné toutes les 33 ms, même inactif.

**Travail synchrone sur le thread de l'interface**
- Session complète sérialisée chaque seconde d'activité, et écrite avec fsync à chaque fermeture d'onglet.
- Copie complète du buffer par point d'annulation.

**Recalculs dans `view()`**
- Arbre de projet : environ 18 `t!()` par ligne.
- Liens FK de la grille.
- Lignes du diff.
- `resolve()` du client API.
- Journal de la console.

**Listes non virtualisées**
- Finder (2 000 lignes), Résultats, arbre de projet, sortie des tâches (5 000 lignes).

**Correctif structurel :**
- surveiller les fichiers (crate `notify`) sur le worktree, `.git/index` et `HEAD` ;
- suspendre les ticks quand la fenêtre n'a pas le focus ;
- redessiner le terminal sur un abonnement par canal ;
- debounce et `spawn_blocking` pour la session et le lint ;
- mettre en cache dans l'état, en invalidant sur `revision` ;
- virtualiser les longues listes.

### T6. Cycle de vie des processus externes
- **LSP :** `didClose` envoyé seulement avant un renommage ; ni `didSave`, ni `shutdown` / `exit` ; un rust-analyzer par crate d'un workspace. (Délai maximal de 30 s par requête, 120 s pour `initialize`, et arrêt d'un serveur dont le démarrage échoue : faits.)

**Correctif structurel :**
- cycle de vie LSP complet ;
- arrêt des serveurs inactifs.

### T7. Notion de « projet de confiance »
Une confirmation « Faire confiance à ce dossier ? », mémorisée par racine (`trusted_folders`), est demandée à l'ouverture d'un fichier d'un dossier qui porte du code à lui (`node_modules/.bin`, `.venv/bin`, `prettier.config.js`, `.simpleedit/tasks.json`, `build.rs`…). Tant qu'il n'est pas approuvé :
- les programmes locaux du lint ne tournent pas ;
- le formateur tourne hors du dossier (pas de `prettier.config.js`) ;
- les tâches de `tasks.json` demandent la confiance au lieu de s'exécuter ;
- rust-analyzer démarre sans scripts de build ni macros procédurales (relancé après confiance) ;
- la restauration d'un dump psql refuse les lignes qui commencent par une méta-commande lançant un programme (`\!`, `\o |`, `\copy … program`) ; le filtre reste contournable (D · 5).

### T8. Encodages et fins de ligne
- Regex sans `crlf(true)` : `$` échoue sur les fichiers CRLF lus du disque.
- Indexation git impossible en CRLF.
- « Pas de saut de ligne final » perdu dans les patches.
- Le finder ignore Windows-1252 et UTF-16.
- Colonnes rustc / tsc (en caractères) utilisées comme des octets.

### T9. Chaînes non traduites
`tests/unit_i18n.rs` vérifie que les clés existent, mais pas l'absence de texte en dur. Restent en anglais :
- environ 20 messages d'erreur du client API (H · F1) ;
- `ON` / `OFF` de Search Everywhere ;
- `"HEAD (Current Branch)"` ;
- `"rows"` dans le journal de la console ;
- les erreurs de session et de fichier (`"read-only file"`, `"no data directory"`) ;
- `"Alt+↑↓"` ;
- un format de date figé `%d/%m/%Y`.

Un test qui cherche les littéraux passés à `text(…)` ou à `set_status(…)` hors `t!()` éviterait la régression.

---

## 5. Fonctionnalités manquantes importantes

Chaque absence a été vérifiée par recherche dans le code. Les plus utiles sont en gras.

### Édition
- **Indentation intelligente** : indenter après `{`, `(`, `:` ; éclater `{|}` avec Entrée ; désindenter en tapant `}` ; Retour arrière qui supprime la paire auto-fermée.
- **Copier ou couper la ligne entière sans sélection** (Ctrl+C / Ctrl+X).
- Regroupement des Retour arrière / Suppr consécutifs dans l'annulation, et état multi-curseurs restauré par l'annulation.
- Complétion par mots du buffer et snippets utilisateur quand aucun serveur de langage n'est disponible.
- Remplacement d'une correspondance vide (`^` → `// `) pour préfixer des lignes par regex.

### Application et fichiers
- **Surveillance des fichiers par événements** (crate `notify`) à la place de la scrutation.
- **Corbeille** au lieu de la suppression définitive.
- **Renommer ou déplacer un dossier avec onglets, signets et historique qui suivent.**
- « Recharger depuis le disque » / « Revenir à la version enregistrée », « Tout enregistrer ».
- Indication d'un fichier supprimé ou renommé sur le disque.
- Réouverture d'un « Sans titre » fermé.
- Parcours qui respecte `.gitignore` même sans git (crate `ignore`), et prise en compte des sous-modules.
- Bouton d'annulation et progression des recherches longues (seule la recherche dans les fichiers s'arrête d'elle-même quand la requête change).

### Git
- **Saisie des identifiants** (`GIT_ASKPASS` / `SSH_ASKPASS` graphique). Sans helper, un push HTTPS est impossible.
- Clone d'un dépôt.
- Récupération via le reflog après un reset, un `-D` ou un drop.
- Sous-modules et worktrees.
- Options de stash (`--keep-index`, `--staged`, vers une branche), `git clean` avec aperçu.
- Blame calculé sur le buffer (`--contents -`).

### Bases de données
- **Stop ou délai maximal pour les opérations de la grille** (la console MySQL s'arrête maintenant avec `KILL QUERY`).
- **Test de santé des sessions PostgreSQL** (MySQL : ping avant réutilisation, fait).
- **Troncature des valeurs volumineuses côté pilote** et chargement à la demande (l'affichage de la grille est déjà tronqué).
- Import, designer, vidage et suppression pour MySQL et SQLite.
- Empreinte de la clé d'hôte SSH affichée et confirmée.
- Collage d'un bloc TSV dans la grille.
- Réutilisation et épinglage des onglets de résultat.
- `COUNT(*)` total, filtre « par cette valeur », paramètres de requête dans la console.

### LSP, terminal, tâches
- **Cycle de vie complet des documents** (`didClose`, `didSave`, `didChangeWatchedFiles`) et annulation des requêtes.
- **Plan du fichier** (`documentSymbol`), `typeDefinition` / `implementation`.
- Actions de code `refactor` et `source.organizeImports`, `workspace/executeCommand`, `rangeFormatting`.
- Inlay hints, semantic tokens, documentHighlight.
- Terminal : recherche, liens `fichier:ligne` cliquables, transmission de la souris (vim, htop).
- PATH complété sur macOS lancé depuis le Finder (Homebrew, cargo, nvm).

### Client API
- **multipart/form-data** (`curl -F`, fichiers des `formdata` Postman).
- **Pot de cookies** entre requêtes et pendant un Run.
- **TLS d'entreprise** : AC système (`rustls-tls-native-roots`), AC personnalisée, mTLS, proxy.
- Annulation d'un « Run collection ».
- Héritage d'auth et de variables par collection ou dossier.
- Variables dynamiques (`{{$guid}}`, `{{$timestamp}}`).

### Build et distribution
- Runner ancien pour la compatibilité glibc du `.deb` (la release attend maintenant les tests, compile en `--locked` avec un toolchain épinglé).
- Documentation des patches `vendor/` (`iced_graphics` est patché sans que ce soit signalé), avec un test qui échoue si le patch n'est plus appliqué.
- Crates séparées pour les pilotes : `postgres`, `mysql` et `rusqlite` (C compilé) ne servent qu'aux binaires `drivers/` mais sont compilés avec l'éditeur.
- Remplacer `serde_yaml` (abandonné) par `serde_norway` ou `serde_yaml_ng`.
- Job CI avec PostgreSQL et MySQL pour les tests des pilotes.

---

## 6. Plan de correction proposé

**Phase 1 : arrêter les pertes de données et les failles (P0)** : **terminée**, ainsi que 10 points P1. Les 15 points ont été corrigés, avec leurs tests de non-régression (multi-curseurs, mode grand fichier, gardes de fermeture, chemins littéraux Git, confirmations Git, troncature `varchar`, trousseau illisible, tunnel SSH, découpeur SQL par dialecte, collage du terminal, redirections et secrets de l'API, projets de confiance).

**Phase 2 : robustesse structurelle**
- T1 : identifiants de requête pour les dernières réponses sans cible.
- T3 : un seul chemin d'écriture.
- T4 : un designer MySQL et SQLite.
- T6 : processus externes (LSP, tâches, Stop MySQL).

**Phase 3 : performance (T5)**
- Surveillance par `notify`.
- Ticks suspendus hors focus.
- Session et lint hors du thread de l'interface (l'orthographe des gros fichiers y est déjà).
- Annulation par deltas.
- Virtualisation des listes.
- Caches de `view()`.

**Phase 4 : fonctionnalités (section 5)**
Commencer par celles qui réduisent les risques : corbeille, saisie des identifiants Git, test de santé des sessions PostgreSQL.

---

# Annexes : rapports détaillés par domaine

Chaque annexe reprend l'audit complet d'un domaine : bugs, problèmes fonctionnels, performance, sécurité et fonctionnalités manquantes, avec emplacement, sévérité, scénario et correctif.

---

## Annexe A — Cœur de l'éditeur (widget, multi-curseurs, annulation, repli, recherche, complétion, orthographe)

### Bugs

### Problèmes fonctionnels

### Performance

#### 2. [haute] Copie complète du tampon pour presque chaque point d'annulation
- **Où** : `src/app.rs:5852-5864` et `src/history.rs:67-78`, `115-130`.
- **Problème** : `push_undo` fait `buffer_text` (copie entière) puis `compress`, qui découpe deux fois tout le texte en `Vec<&str>`. Cela arrive à chaque Entrée, collage et début de mot (les Retour arrière / Suppr consécutifs sont regroupés). L'état multi-curseurs n'est pas non plus restauré par l'annulation (`snapshot` ne garde que le curseur principal). L'édition complète est permise jusqu'à 64 Mo (`MAX_OPEN_BYTES`).
- **Scénario** : fichier de 30 Mo, maintenir Retour arrière → plusieurs passes de 30 Mo par frappe.
- **Correctif** : enregistrer des deltas au niveau des éditions (plage + texte remplacé) au lieu d'instantanés.

#### 4. [moyenne] Pliage et minimap re-parcourent tout le fichier à chaque frappe [partiel : minimap incrémentale ; reste le calcul des régions de pliage]
- **Où** : `src/code_editor.rs:1358-1373` (`Regions::find`, jusqu'à 100 k lignes, lancé à chaque `layout` quand `revision` change) et `src/code_editor.rs:716-723` (`minimap_span` sur toutes les lignes, sans limite, minimap activée par défaut).
- **Correctif** : différer ou rendre incrémental (ne refaire qu'à partir de `changed_from`).

### Fonctionnalités manquantes (absence vérifiée par grep)

1. **Indentation intelligente** : pas d'indentation supplémentaire après `{`, `(`, `:`, pas d'éclatement de `{|}` avec Entrée, pas de désindentation en tapant `}`. Retour arrière juste après `(` ne supprime pas le `)` ajouté automatiquement. Entrée ne fait que recopier l'indentation (`app.rs:5573-5600`). [partiel : indentation après `{ ( [` et éclatement d'une paire faits ; reste la désindentation en tapant `}` et le Retour arrière sur une paire]
3. **Complétion sans serveur de langage** : aucune proposition par mots du tampon ni snippets utilisateur ; sans serveur, il n'y a aucune liste (`app/completion.rs:1-7`).
---

## Annexe B — Application : cycle de vie des fichiers, session, onglets, sauvegarde automatique, grands fichiers

**Bilan :** 6 bugs (1 moyen, 5 bas), 5 problèmes fonctionnels, 6 problèmes de performance et 5 fonctionnalités manquantes. Le plus grave qui reste : un grand fichier qui grossit revient à sa première partie chaque seconde (bug 5).

Tous les fichiers du périmètre ont été lus en entier (liste à la fin).

---

### Bugs

Aucun point restant (tous corrigés).

---

### Problèmes fonctionnels

#### 5. Basse : un fichier supprimé sur disque n'est signalé nulle part [partiel : signalé pour le fichier courant seulement]
**Où :** `src/app.rs:7623-7625`.

**Le problème :** quand `DiskStamp::of` renvoie `None`, la vérification s'arrête en silence, et la sauvegarde suivante recrée le fichier. Les onglets en arrière-plan ne sont vérifiés qu'au moment où on les active.

---

### Performance

#### 1. Moyenne : un minuteur d'une seconde tourne en permanence, avec des appels git même à l'arrêt [partiel : pause sans focus et scrutation à 2 s ; le minuteur d'une seconde reste, pas de watcher `notify`]
**Où :** `src/app.rs:1485-1486` et `4351-4415`.

À chaque seconde, même fenêtre inactive : un `update` et un `view()` complets, plus des `stat` des dossiers dépliés sur le thread de l'interface (`app.rs:4359`). En plus, via des tâches :
- `git status` de toutes les racines toutes les 3 s (`app.rs:4360`) ;
- `GitMsg::Tick` toutes les 5 s (`app.rs:4380-4387`) ;
- `git show HEAD:file` toutes les 10 s (`app.rs:4372`).

C'est un coût CPU et batterie réel, surtout sur de gros dépôts.

**Correction :** suspendre ces tâches quand la fenêtre n'a pas le focus ou après une période d'inactivité, surveiller les fichiers avec `notify` plutôt que d'interroger git, et espacer les intervalles.

#### 2. Moyenne : la session est construite sur le thread de l'interface à chaque seconde d'activité [partiel : au plus tous les 3 ticks ; la construction reste sur le thread UI]
**Où :** `src/app/views.rs:1084-1094`.

Tout message non passif incrémente `rev`. Le texte de tous les buffers Sans titre ou modifiés est alors copié, sérialisé en JSON et haché. Un Sans titre de 30 Mo provoque un à-coup visible chaque seconde, même pendant qu'on travaille ailleurs.

**Correction :** ne re-sérialiser que les buffers qui ont changé, construire la session hors du thread de l'interface, et regrouper les écritures sur 3 à 5 s.

#### 5. Basse : des accès disque dans `view()` [partiel : reste `exists()` de la fenêtre d'historique, tri de `Ext::all()`, clone du changelog]
- Fenêtre d'historique local : un `exists()` / `is_dir()` par ligne et par frame (`app/local_history.rs:542`, `586`, `603`).
- Panneau scratch : `next_name` fait une boucle de `stat` (`views.rs:443`, `scratch.rs:64-68`), et `Ext::all()` est trié à chaque frame.
- Changelog : cloné à chaque frame (`changelog.rs:65-70`).

**Correction :** précalculer ces valeurs dans `update`.

#### 6. Basse : l'historique local grossit sans borne de temps [partiel : plafond de 365 jours ; `history_open` lit encore tous les index sur le thread UI]
**Où :** `src/local_history.rs:277-281`, `184-193`, `293`.

- La conservation compte les « jours travaillés » propres à chaque fichier : un fichier modifié une seule fois garde ses révisions jusqu'au plafond de 512 Mo.
- `history_open` lit tous les index sur le thread de l'interface (`app/local_history.rs:164`).
- `prune` refait le `stat` de tous les blobs à chaque itération, soit un coût quadratique (en arrière-plan).

---

### Fonctionnalités manquantes (absence confirmée par recherche)

3. **Indication d'un fichier supprimé ou renommé sur disque.**
4. **Récupération d'un Sans titre fermé** : `closed_tabs` exclut les Sans titre (`app.rs:7058`).
5. **Surveillance des fichiers par événements** (`notify` absent de `Cargo.toml`) à la place de l'interrogation chaque seconde.

---

Fichiers lus en entier : `src/app.rs`, `src/main.rs`, `src/app/file_io.rs`, `src/session.rs`, `src/config.rs`, `src/tabs.rs`, `src/tab_strip.rs`, `src/app/split.rs`, `src/app/tab_groups.rs`, `src/app/views.rs`, `src/fsutil.rs`, `src/local_history.rs`, `src/app/local_history.rs`, `src/scratch.rs`, `src/bookmarks.rs`, `src/app/bookmark_panel.rs`, `src/app/compare.rs`, `src/changelog.rs`, `src/menu_bar.rs`, `src/native_menu.rs`, `src/floating.rs`, `src/pointer_area.rs`. J'ai aussi consulté, pour suivre les appels, `sidebar/mod.rs`, `app/project_ops.rs`, `history.rs` et `projects.rs`.

---

## Annexe C — Intégration Git

**Méthode.** J'ai lu tout le code de production du périmètre et les gestionnaires Git de `src/app.rs` ; les modules de tests ont été survolés. Sept points ont été reproduits dans des dépôts git jetables, avec git 2.53 : le CRLF, l'absence de fin de ligne, `diff.noprefix`, le commit de merge vide, les pathspecs en glob, l'ambiguïté de `checkout` et la locale française.

Deux pistes ont été écartées après vérification :
- **Locale française.** `[rejected]` et `%(upstream:track)` de `for-each-ref` ne sont pas traduits, donc `is_rejected` et `parse_refs` restent fiables.
- **`checkout <nom>` sur une branche distante ambiguë.** git échoue (« matched multiple remote tracking branches ») au lieu de restaurer un chemin du même nom.

---

### Bugs

#### 21. Invites SSH ou GPG invisibles — **faible [plausible]** [partiel : auto-fetch en BatchMode ; reste un askpass graphique pour les push]
- **Où :** `src/git/mod.rs:27-41`.
- **Problème :** `GIT_TERMINAL_PROMPT=0` ne couvre pas `ssh`, qui lit `/dev/tty` (passphrase, clé d'hôte inconnue), ni un `pinentry-curses`.
- **Scénario :** application lancée depuis un terminal : un push attend jusqu'à 900 s, avec `busy` bloqué.
- **Pour confirmer :** lancer depuis un terminal avec une clé protégée et sans agent.
- **Correctif :** `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` pour l'auto-fetch, et un `SSH_ASKPASS` / `GIT_ASKPASS` graphique pour le reste.
- **Variante [plausible] :** si SimpleEdit démarre comme `core.editor` de `git commit`, il hérite de `GIT_INDEX_FILE`. Il faudrait retirer `GIT_DIR`, `GIT_WORK_TREE` et `GIT_INDEX_FILE` de l'environnement des commandes.

---

### Problèmes fonctionnels

---

### Performance

1. **Polling permanent sans garde « en vol » — moyenne.** [partiel : le minuteur du panneau attend la réponse précédente ; reste celui de 3 s panneau fermé]
   - Panneau ouvert, toutes les 2 s (`app.rs:1578-1583`, `git_panel/mod.rs:931-940`) : `rev-parse`, `status --untracked-files=all`, `git diff` du fichier ouvert, plus un diff par onglet de diff.
   - Toutes les 3 s, même panneau fermé (`app.rs:4359-4363`, `git/mod.rs:155-178`) : `rev-parse`, puis `status --ignored=matching -uall` par dépôt.
   - Si un `status` dure plus que l'intervalle (gros monorepo), les processus s'empilent.
   - **Correctif :** watcher (`notify`) sur `.git/index`, `HEAD` et le worktree, plus un drapeau « status en cours ».
3. **Travail O(n) dans `view()` à chaque frame — moyenne.** [partiel : hauteurs et `longest` en cache ; `side_rows()`, `rows()`, `row_blocks()`, `partners()` restent par image]
   - **Où :** `diff_view.rs:668-671`, `801-809`, `1185-1187`.
   - **Problème :** `side_rows()` est calculé deux fois, `side_heights` (parcours caractère par caractère de tout le diff), `rows()`/`row_blocks()`/`partners()`, ainsi que le calcul de `longest`. Pendant un glisser, `index_patch(None)` construit en plus le patch complet. Tout cela à chaque mouvement de souris et chaque scroll.
   - **Correctif :** mettre en cache dans `Viewer` (invalidé au changement de diff, de largeur ou de sélection).
4. **Log coûteux — faible.** [non fait : demande de changer `GitMsg::LogLoaded`]
   - Chaque `show_log` relance `--topo-order` et `rev-list --max-count=5000`, plus `graph::layout` sur le thread UI (`history.rs:126-131`, `log.rs:280` et `305`).
   - « Charger plus » refait tout depuis le début.
   - [plausible] Sans fichier commit-graph, `--topo-order` parcourt tout l'historique.
---

### Sécurité

Je n'ai rien trouvé d'exploitable par injection d'arguments :
- les révisions saisies passent par `verify_rev` ;
- les refs utilisent `--end-of-options` ;
- les chemins sont placés après `--` ;
- les noms de branche commençant par `-` sont refusés par git.

Le comportement glob des pathspecs est neutralisé (`GIT_LITERAL_PATHSPECS=1`, `:(literal)` pour `git stash`).

---

### Fonctionnalités manquantes (par priorité)

1. **Saisie des identifiants (`GIT_ASKPASS`, passphrase SSH).** Avec `GIT_TERMINAL_PROMPT=0` et aucun askpass, un push ou un pull HTTPS sans credential helper est impossible : « terminal prompts disabled ».
2. **Clone d'un dépôt** : aucune occurrence de `clone`.
3. **Récupération via le reflog** (« annuler » un reset `--hard`, un `-D` ou un drop) : absente, alors que ces opérations destructives existent.
4. **Sous-modules et worktrees** : aucun support (init/update, liste).
5. **Options de stash** (`--keep-index`, `--staged`, stash → branche) et **`git clean`** avec aperçu. [partiel : `--keep-index` et stash vers une branche faits ; restent `--staged` et `git clean`]
6. **Signature GPG ou SSH** : pas d'option, pas d'affichage de la vérification. On dépend uniquement de `commit.gpgsign` côté configuration.

---

**Fichiers lus en entier** (hors modules de tests) : `src/git/mod.rs`, `src/git/history.rs`, `src/git/tools.rs`, `src/git/diff.rs`, `src/git/conflicts.rs`, `src/git/graph.rs`, `src/git/marks.rs`, `src/git_panel/{mod,view,diff_view,log,log_view,branches,merge,conflicts_view,stash,stash_view,tools,diff_syntax,icon}.rs`, `src/app/git_file.rs`, et les sections Git de `src/app.rs` (1560-1590, 2590-2880, 4300-4410, 6240-7010, 7318-7335).

---

## Annexe D — Base de données, back-end (pilotes, protocole, tunnel SSH, TLS, trousseau)

J'ai lu en entier tous les fichiers du périmètre, et suivi les appelants jusque dans `src/app/database.rs`, `grid.rs`, `console.rs` et `updater.rs`. Les problèmes les plus graves qui restent : des clés binaires MySQL impossibles à éditer (13), et une reconnexion PostgreSQL silencieuse au milieu d'une transaction (20).

**Ce qui est correct** (vérifié) :
- PostgreSQL `verify-full` vérifie bien le nom d'hôte, y compris à travers le tunnel grâce à `hostaddr` (postgres/main.rs:394-397, 563).
- `require` ne vérifie pas le certificat, ce qui correspond à la sémantique de libpq.
- Les mots de passe passent par stdin ou par l'environnement, jamais par les arguments du processus.
- Le téléchargement des pilotes s'appuie sur un `SHA256SUMS` signé et sur un nom d'asset qui contient la version, donc pas de rejeu d'une ancienne release. Si la vérification échoue, rien n'est écrit sur le disque.
- Pas d'interblocage de pipes : stderr va vers `null`, et la requête est lue en entier avant la réponse.
- Les modifications du grid passent par une session (`browse`) distincte de celle de la console. Elles ne peuvent donc ni valider ni annuler la transaction ouverte dans la console.

---

### Sécurité

---

### Bugs

**18. [moyenne] La session browse n'a ni limite de temps ni bouton Stop, et les éditions de résultats de la console passent par elle** [partiel : limite de temps de 600 s ; reste le bouton Stop et les éditions `live` par la session console]
`mod.rs:1010-1022,1116,1295-1298`
- **Problème.** Avec `query_timeout=0`, `rows`, `apply` et `export` attendent sans limite en tenant le mutex de la session browse.
- **Scénario.** Dans la console : `BEGIN; UPDATE t … WHERE id=5; SELECT * FROM t`. On édite la ligne 5 dans l'onglet résultat : `apply` passe par la session browse, attend le verrou de la console, et tout l'arbre et tous les onglets de la source sont gelés. Une ligne insérée sans commit dans la console renvoie « 0 rows touched ».
- **Correctif.** Faire passer les éditions des onglets `live` par la session console. Mettre un `lock_timeout` et un Stop sur la session browse.

**21. [faible] Annulation PostgreSQL derrière PgBouncer en mode transaction** *[plausible]*
`postgres/main.rs:159-161,227-235`
- **Problème.** `pg_cancel_backend(pid)` vise un backend qui peut avoir été réattribué à un autre client (cas des URL `-pooler` de Neon).
- **Correctif.** Utiliser `client.cancel_token()`, l'annulation au niveau du protocole, que PgBouncer route correctement.

**22. [faible] Défauts TLS MySQL [partiel : certificat client et repli `prefer` faits, `verify-full` par tunnel vérifie la chaîne mais pas le nom]**
`mysql/main.rs:78-98`
- `prefer` se comporte comme `require` et échoue sur un serveur sans TLS.
- `client_cert` et `client_key` sont ignorés sans avertissement.
- `verify-full` à travers le tunnel vérifie le certificat contre `127.0.0.1`, donc échoue toujours.

---

### Performance

**26. [moyenne] Un processus lancé à chaque requête**
`mod.rs:1045,1112`
- **Problème.** `find_driver_for` exécute `pilote --protocol` à **chaque** requête, même quand le processus `--serve` existe déjà : un fork+exec par page, tri ou rafraîchissement.
- **Correctif.** Mettre le chemin en cache par dialecte, et ne le résoudre qu'au lancement du processus.

**27. [moyenne] Protocole PostgreSQL bavard** [partiel : `source_of` en cache ; `in_transaction` ajoute encore un aller-retour]
`postgres/main.rs:241-250,1302-1306`
- **Problème.** `in_transaction` ajoute un aller-retour après chaque requête de la console, et `source_of` en ajoute 3 par page de résultat (≈ 150 ms à 50 ms de latence via SSH).
- **Correctif.** Mettre `source_of` en cache par SQL.

**28. [moyenne] Résultats chargés entièrement** [partiel : exports sql et md en flux ; reste xlsx, la lecture PostgreSQL et la fin de lecture MySQL]
- `postgres/main.rs:2409` : `simple_query` matérialise *toutes* les lignes d'un statement non paginable (`FOR UPDATE`, `SHOW`, lot multiple) avant d'en garder 1000.
- `mysql/main.rs:160-165` : le `break` fait vider le reste du résultat par le `Drop` de `QueryResult`, qui lit tout le résultat sur le réseau.
- `mod.rs:1729-1735` : les exports sql/md/xlsx lisent tout le CSV en mémoire.
- **Correctif.** Utiliser `query_raw` et une portée limitée côté PostgreSQL, et convertir en flux.

**29. [moyenne] Aucune limite sur la taille des valeurs** [partiel : la réponse du pilote est plafonnée à 256 Mo ; reste la troncature côté pilote avec chargement à la demande]
`mod.rs:44,1148`
- **Problème.** Une page de 5000 lignes avec des `bytea`/`text` de plusieurs Mo produit une seule ligne JSON de plusieurs Go, gardée par le pilote puis par `read_line`.
- **Correctif.** Tronquer côté pilote (`left(col::text, N)`) et charger la valeur complète à la demande.

**31. [faible] Trousseau interrogé sur le thread UI** [partiel : écriture en thread séparé ; le chargement au démarrage reste synchrone]
`mod.rs:763-827`, `app/database.rs:1059`
- **Problème.** `persist()` fait 2 appels D-Bus ou Keychain par connexion sur le thread UI, à chaque changement de taille de page, et le chargement au démarrage est synchrone.

---

### Fonctionnalités manquantes (par priorité, absence vérifiée)

1. **Stop/timeout pour les opérations du grid** (rows, apply, export, import). Aujourd'hui seule la console est annulable (`app/database.rs`), y compris sur MySQL (`KILL QUERY`).
2. **Contrôle de santé des sessions PostgreSQL** avant réutilisation (voir 20) ; MySQL vérifie déjà sa session (ping) et refuse de rouvrir en silence au milieu d'une transaction.
3. **Import CSV/JSON et copie de table pour SQLite et MySQL**. `common/mod.rs:201` renvoie « not available ».
4. **Valeurs volumineuses** : troncature et chargement paresseux des LOB dans le grid (voir 29).
5. **SSH** : affichage et confirmation de l'empreinte de l'hôte, `ServerAliveInterval`, saut d'hôte.
6. **MySQL** : certificats client, édition des tables sans PK via une clé UNIQUE NOT NULL. **SQLite** : vrai timeout de requête (`interrupt`, et non `busy_timeout`).

---

Fichiers lus en entier : `drivers/common/mod.rs`, `drivers/common/tunnel.rs`, `drivers/postgres/main.rs`, `drivers/mysql/main.rs`, `drivers/sqlite/main.rs`, `src/database/protocol.rs`, `src/database/mod.rs` (les tests en fin de fichier parcourus), `src/database/secrets.rs`, `src/database/import.rs`, `src/database/convert.rs`, `src/database/ddl.rs`, `tests/sql_drivers.rs`.

---

## Annexe E — Base de données, interface (grille, console, designer, complétion SQL, diagramme)

Chaque constat ci-dessous a été vérifié en lisant le code, appelants compris, sauf ceux marqués [plausible].

### Bugs

**11. Moyenne : les superpositions de la console ignorent le défilement et le retour à la ligne.** [partiel : plus de retour à la ligne ; reste le défilement, que iced 0.14 n'expose pas] `src/database/view.rs:2906-2977`, `2855-2870`
- **Problème :**
  - Le cadre de l'instruction, le soulignement d'erreur et la liste de complétion sont placés à `ligne logique × 16.9 px` depuis le haut.
  - Le `text_editor` défile et coupe les lignes (`Wrapping::Word` par défaut).
  - Le raccourci capture Entrée, Tab et les flèches dès que `completion.is_some()`, même si la liste est dessinée hors de la zone visible.
- **Scénario :**
  - Console de 80 lignes défilée vers le bas : on tape `sel` puis Entrée. « SELECT » est inséré par une liste invisible au lieu d'un saut de ligne.
  - Le cadre encadre la mauvaise instruction.
- **Correctif :** désactiver le retour à la ligne et suivre le défilement via `Action::Scroll`, ou ne pas afficher et ne pas capturer quand la position calculée sort de la zone visible.

### Problèmes fonctionnels

### Performance

2. **Moyenne : coûts par image dans la grille.** `src/database/view.rs:2233-2243`, `2311`, `2244` [partiel : liens FK à la demande, `PickKind` par colonne ; reste la virtualisation des colonnes]
   - Pour chaque ligne visible, à chaque `view()` (y compris chaque message `Scrolled`), `ddl::links` re-parse toutes les FK et références, avec un `Arc<Related>` et des clones de chaînes.
   - `PickKind::of` fait un `to_lowercase` par cellule.
   - Pas de virtualisation horizontale : 200 colonnes × 40 lignes × environ 6 widgets par cellule.
   - Correctif : calculer les liens à la demande dans la fermeture du menu contextuel, précalculer `PickKind` par colonne dans `set_page`, et ne construire que les colonnes visibles.
3. **Moyenne : complétion recalculée à chaque frappe.** [partiel : `known()` est mis en cache ; reste la tokenisation de tout le texte dans `suggest()`] `src/app/database.rs:143-163`, `src/database/mod.rs:2429-2468`
   - `State::known()` reclone toutes les colonnes (jusqu'à 100 000) dans une `HashMap`, plus une boucle O(tables²) en `2463-2467`.
   - `sql_tokens` tokenise tout le texte de la console.
   - Correctif : mettre `Known` en cache par connexion (invalidé sur `TreeLoaded`/`ColumnsLoaded`) et ne tokeniser que l'instruction courante.
4. **Moyenne : coûts par image dans la console.** `src/database/view.rs:2906`, `src/database/console.rs:384-397`, `src/database/view.rs:2746-2831` [partiel : découpage des instructions en cache ; le journal n'est pas virtualisé]
   - `statement_lines()` copie et redécoupe tout le texte de la console à chaque `view()`.
   - Le journal construit jusqu'à 1 000 entrées (SQL complet) sans virtualisation.
   - Correctif : mémoriser le découpage tant que le texte n'a pas changé, et virtualiser le journal.
6. **Note pour le back-end :** `rows_with` n'utilise pas `params.browse` (`mod.rs:1285`). Chaque page lance un processus et une connexion, tunnel SSH compris. C'est lent.

### Fonctionnalités manquantes

Absence confirmée par recherche dans le code.

2. **Réutilisation et épinglage des onglets de résultat** (voir le point fonctionnel 3).
4. **Menu de cellule « Filtrer par cette valeur »**, et masquer des colonnes.
5. **Paramètres de requête** (`:nom`, `?`) dans la console.
6. **Designer, vider, supprimer et importer pour MySQL et SQLite** : les menus sont volontairement limités à PostgreSQL (`view.rs:515`, `3166`).

Fichiers lus entièrement : `src/database/view.rs`, `src/database/grid.rs`, `src/database/designer.rs` (tests survolés), `src/database/console.rs`, `src/database/completion.rs`, `src/database/admin_view.rs`, `src/database/diagram.rs`, `src/app/database.rs`, `src/app/db_tools.rs`, `src/app/db_tree.rs`, `src/app/table_admin.rs` ; plus les parties utiles de `src/database/mod.rs`, `src/database/protocol.rs` et `src/app.rs`.

---

## Annexe F — Outils externes (LSP, lint, terminal, tâches, formateur, fichiers SSH)

J'ai lu en entier tous les fichiers du périmètre, ainsi que leurs appelants dans `src/app.rs` et `src/app/completion.rs`. Les problèmes les plus graves qui restent : `didSave` jamais envoyé (bug 10), un rust-analyzer par crate d'un workspace (performance 2) et la sortie d'une tâche coupée au premier octet non UTF-8 (bug 8).

---

### Bugs

### Sécurité

**Vérifié sans problème :** `remote.rs` met correctement entre guillemets tous les chemins (`'…'\''…'`, avec `--`). `valid_host` refuse un `-` en tête et les espaces. Les outils de lint reçoivent leurs arguments sans passer par un shell, et `{file}` est absolu.

---

### Problèmes fonctionnels

---

### Performance

#### 2. [Moyenne] Un serveur par marqueur de projet le plus proche, jamais arrêté [partiel : un rust-analyzer par workspace ; reste l'arrêt des serveurs inactifs et les autres langages] [serveurs inactifs arrêtés après 20 min ; reste un serveur par dossier pour les autres langages]
`src/lsp.rs:601-603`, `1478-1486`, `src/definition.rs:77-85`

- **Scénario :**
  - Dans un workspace Cargo, chaque `crates/*/Cargo.toml` démarre son propre rust-analyzer, qui charge tout le workspace. Ouvrir 5 crates donne 5 instances de 1 à 3 Go.
  - Pour des fichiers isolés, on obtient un pyright par dossier.
- **Correction :** pour Rust, remonter jusqu'à la racine du workspace (`cargo locate-project --workspace` ou le `.git` le plus haut). Arrêter les serveurs restés inactifs N minutes.

#### 5. [Basse] Copies du texte complet à chaque frappe [partiel : réallocation évitée ; reste la synchronisation par modifications]
- **Ce qui se passe :** `buffer_text` et les copies dans `docs` font plusieurs copies de tout le texte, plus des comparaisons sur tout le texte, à chaque frappe.
- **Correction :** synchroniser à partir des modifications de l'éditeur.

---

### Fonctionnalités manquantes (par priorité, absences vérifiées par grep)

1. **Cycle de vie des documents LSP :** `didClose` (envoyé seulement avant un renommage), `didSave`, `didChangeWatchedFiles`. C'est la cause du bug 10. [partiel : `didClose` et `didSave` faits ; reste `didChangeWatchedFiles`]
2. **Annulation** des requêtes LSP par l'utilisateur (un délai maximal existe : 30 s, 120 s pour `initialize`).
3. **`textDocument/documentSymbol`** (plan du fichier) et `typeDefinition`/`implementation` : absents.
4. **Actions de code au-delà de « quickfix » au survol :** `refactor` et `source.organizeImports` absents. Pas de `workspace/executeCommand` : `workspace/applyEdit` reçoit `null`, ce qui n'est pas une réponse valide.
5. **Formatage de sélection via LSP :** `rangeFormatting` absent. « Formater la sélection » échoue pour les langages qui n'ont qu'un serveur (php, java, etc.).
6. **Terminal :** recherche dans l'historique, liens `fichier:ligne` cliquables, transmission de la souris (vim, htop). Pas de réponse aux requêtes DSR/DA (selon le crate vt100 ; plausible, non confirmé).
7. **Inlay hints, semantic tokens, documentHighlight :** absents.
8. **Tâches :** pas de rapprochement entre sortie et diagnostics dans le panneau Problèmes.

---

**Fichiers lus entièrement :** `src/lsp.rs` (le code et le serveur de test ; les tests après la ligne 2100 seulement parcourus), `src/lsp_install.rs`, `src/lint/mod.rs`, `src/lint/parse.rs`, `src/lint/run.rs`, `src/lint/tools.rs`, `src/app/lint.rs`, `src/terminal.rs`, `src/tasks.rs`, `src/app/tasks.rs`, `src/formatter.rs`, `src/remote.rs`, `src/app/remote_files.rs`. Plus les extraits appelants de `src/app.rs` (rename, hover, format, save, terminal, définition), `src/app/completion.rs`, `src/definition.rs:54-85` et `src/config.rs` (valeurs par défaut).

---

## Annexe G — Recherche dans les fichiers, Search Everywhere, projets, barre latérale

J'ai lu tout le code, appelants compris. Deux comportements ont aussi été vérifiés hors code : `git ls-files` sur un dépôt imbriqué temporaire, et la sémantique documentée du crate `regex` 1.13.

---

### Bugs

---

### Problèmes fonctionnels

**F2 — moyenne — Suppression définitive, sans corbeille** (`src/app/project_ops.rs:237`, `src/app.rs:8786`)
- **Constat :** `remove_dir_all` / `remove_file` sont appelés directement. Le filet de l'historique local s'arrête à 2000 fichiers et saute `node_modules` (`local_history.rs:21,162`).
- **Comparaison :** VS Code et JetBrains envoient à la corbeille par défaut.
- **Correctif :** crate `trash`, avec une option de suppression définitive.

**F3 — faible [partiel : la confirmation signale le plafond, reste l'exclusion d'une occurrence] — Plafond de 2000 occurrences peu visible au remplacement** (`src/finder.rs:267`, `src/app/finder.rs:1531-1535`)
- **Constat :** « Tout remplacer » ne traite que les 2000 premières. La confirmation dit « Remplacer 2000 occurrences dans N fichiers ? » sans rappeler la troncature, et la note « Les 2000 premiers endroits » est en bas de la liste.
- **Autre point :** on ne peut pas exclure une occurrence dans la fenêtre du finder ; il faut l'envoyer au panneau Résultats.
- **Correctif :** mentionner la troncature dans la confirmation et permettre Suppr/Exclure dans la fenêtre.

---

### Performance

**P3 — moyenne-élevée — Classement des fichiers sur le thread UI à chaque frappe et à chaque résultat asynchrone** (`src/app/everywhere.rs:600-622`, `765-853` ; `src/finder.rs:188-251`) [partiel : requête préparée une fois, sélection partielle ; le classement reste sur le thread UI]
- **Problème :** `score` réalloue par fichier le `Vec<char>` de la requête, deux `Vec<char>` du chemin et deux `String`. `file_type()` alloue aussi par fichier. Toutes les correspondances sont clonées puis triées en entier.
- **Déclencheurs :** `everywhere_refresh` est relancé par chaque frappe, chaque réponse LSP, et toutes les 800 ms tant que le serveur est occupé.
- **Correctif :** préparer la requête une seule fois, faire une sélection partielle (`select_nth_unstable`) avant de cloner, et déporter le calcul (ou utiliser `nucleo`) au-delà de N fichiers.

**P4 — moyenne — L'arbre projet est entièrement reconstruit à chaque `view()`** (`src/sidebar/mod.rs:568-658`, `src/projects.rs:472-513`) [partiel : libellés du menu à l'ouverture ; `projects.rows()` clone encore, pas de virtualisation]
- **Problème :** `rows()` clone toutes les entrées visibles. Chaque ligne crée un `ProjectMenuLabels::new` (18 appels `t!()` et 18 `String`), un tooltip, un menu contextuel et une mouse_area. `marks.git` / `has_errors` parcourent les listes d'ignorés et de fichiers lint par ligne.
- **Fréquence :** au moins toutes les secondes (tick) et à chaque frappe dans l'éditeur.
- **Correctif :** calculer les libellés une fois par vue (`Rc`), virtualiser par fenêtre de défilement et mettre en cache les marques de dossiers.

**P5 — moyenne — Arbre rafraîchi par scrutation, avec `git` synchrone sur le thread UI** (`src/app.rs:4359`, `src/projects.rs:388-412`, `153-202`, `275-277`) [partiel : pause sans focus ; le `git check-ignore` synchrone de `projects.rs` reste]
- **Problème :** `refresh(false)` fait un `stat` par dossier ouvert chaque seconde. Chaque rechargement lance un processus `git check-ignore` et attend sa fin, de façon synchrone, si `hide_ignored` est actif. `refresh(true)` (création, renommage, suppression) recharge tous les dossiers ouverts, avec un processus git chacun.
- **Correctif :** watcher `notify` et chargement dans `spawn_blocking`.

**P6 — moyenne — Entrées/sorties synchrones sur le thread UI** [non fait : `replace_found` et `copy_all` dépendent de l'état de l'application]
- **Remplacer tout :** `replace_found` (`src/app/finder.rs`) lit puis réécrit chaque fichier (`file_text` + écriture atomique) sur le thread UI. Un remplacement sur 500 fichiers fige l'interface.
- **Dupliquer :** `copy_all` est aussi synchrone.
- **Listes :** la liste du finder construit 2000 lignes `rich_text` à chaque frame (`1463-1476`), sans virtualisation.
- **Correctif :** déporter les lectures et écritures disque, virtualiser la liste.

**P7 — faible — Aperçu lu de façon synchrone** (`src/app/finder.rs:663`)
- **Constat :** l'aperçu lit tout le fichier de façon synchrone à chaque changement de fichier sélectionné (flèches).

---

### Fonctionnalités manquantes (par priorité, absence vérifiée)

1. **Annulation réelle des recherches longues** (bouton Annuler, Search Everywhere, Rerun), avec un indicateur d'avancement. Seule la recherche dans les fichiers s'arrête quand la requête change.
2. **Watcher de fichiers** : pas de `notify` dans `Cargo.toml` ; tout repose sur la scrutation du tick d'une seconde.
3. **Corbeille** : pas de crate `trash` ; la suppression est définitive.
4. **Renommer/déplacer avec onglets qui suivent** (et signets, historique local), y compris les onglets modifiés. Aujourd'hui l'opération est refusée ou les onglets sont fermés (B8).
5. **Parcours avec `.gitignore` sans git et prise en compte des sous-modules** : le crate `ignore` est absent. Aujourd'hui, `git ls-files` sinon une liste HEAVY codée en dur.
6. **Listes virtualisées** (finder, Résultats, arbre) pour les gros volumes.

---

Fichiers lus en entier : `src/search/mod.rs`, `src/finder.rs`, `src/app/finder.rs`, `src/everywhere.rs`, `src/app/everywhere.rs`, `src/palette.rs`, `src/app/results.rs`, `src/app/compare.rs`, `src/projects.rs`, `src/app/project_ops.rs`, `src/sidebar/mod.rs`, `src/sidebar/icons.rs`, `src/file_icons.rs`. Lus en partie : `src/find_replace.rs` (270-560), et les appelants concernés dans `src/app.rs`, `src/tabs.rs`, `src/editor/mod.rs`, `src/code_editor.rs`, `src/floating.rs`, `src/bookmarks.rs`, `src/app/local_history.rs`.

---

## Annexe H — Sécurité, mise à jour, CI, dépendances, client API, aperçus, préférences

**Points vérifiés et sains :**
- **Mise à jour :** `verify_strict` Ed25519, version stricte, nom d'asset lié à la version (pas de rollback possible), répertoire 0700 et re-hash par root sous Linux.
- **Pilotes de base de données :** ils réutilisent bien `download_verified`.
- **Workflows :** actions épinglées par SHA, `TAG` passé par l'environnement (pas d'injection `${{ }}` dans `run:`), `pull_request` et non `_target`, permissions minimales, clé de signature absente des étapes tierces.
- **`install.sh` :** il refuse d'installer sans vérification, sauf avec `--insecure`.
- **Fichiers privés :** `session.json` et les fichiers de l'API sont en 0600.
- **Preview Markdown :** aucune image distante chargée (pas de pistage), HTML affiché comme texte, liens externes inertes.
- **Preview JSON :** limite de récursion 128 de serde_json et plafond `MAX_ROWS`.
- **Pile TLS :** pas de doublon (un seul rustls 0.23 / ring ; `openssl` figure dans le lock mais n'est pas compilé, `cargo tree -i openssl-sys` est vide).

---

### Sécurité

**S1. Réglages GitHub — moyenne (reste à faire hors du code).** Le job `release` est maintenant dans l'environnement `release`, mais tant que cet environnement n'a pas de relecteur obligatoire et que `UPDATE_SIGNING_KEY` reste un secret du dépôt, un tag poussé avec un jeton compromis est encore signé.
- **Correctif :** dans *Settings → Environments → release*, exiger un relecteur et y déplacer `UPDATE_SIGNING_KEY` ; ajouter une règle de protection des tags `v*`.

**S7. `install.sh:33-44,65-66` — faible à moyenne.** Sur un macOS sans Homebrew (la seule branche qui télécharge l'archive), seul un `openssl@3`… Homebrew sait vérifier Ed25519 avec `-rawin`. L'installation échoue donc toujours et l'utilisateur est poussé vers `--insecure`.
- **Correctif :** signer aussi avec `ssh-keygen -Y sign` et vérifier avec `ssh-keygen -Y verify`, présent d'origine sur macOS et Linux.

**S11. `src/lsp_install.rs:54-78,193-198` — faible.** Les paquets npm ne sont pas épinglés (`pyright`, `intelephense`…) et `gopls@latest` non plus. `npm install -g` exécute les scripts de cycle de vie.
- **Correctif :** épingler les versions et ajouter `--ignore-scripts` quand le serveur le permet.

### Bugs

### Problèmes fonctionnels

**F2. `Cargo.toml:47` — faible à moyenne.** `rustls-tls` équivaut à webpki-roots seuls, sans magasin système (les pilotes de base de données, eux, utilisent `rustls-native-certs`).
- **Scénario :** une API interne signée par l'AC de l'entreprise échoue. La seule issue est de décocher la vérification TLS.
- **Correctif :** `rustls-tls-native-roots` (ou les deux), plus une option « AC personnalisée ».

### Performance

**Scénario :** une réponse de 20 Mo avec 10 tests donne environ 12 parse ou clones de 20 Mo, soit un gel de plusieurs secondes.
**Correctif :** traiter dans `Task::perform` / `spawn_blocking`, parser une seule fois, parcourir par référence.

**P5. `src/api/openapi.rs:46-110,13-24` — faible à moyenne [plausible].** `example()` n'a ni mémoïsation ni détection de cycle : il développe chaque propriété sur 7 niveaux. Un schéma `Node` à 20 propriétés auto-référencées donne environ 20⁷ nœuds, donc un blocage ou un manque de mémoire. Le fichier est aussi parsé 3 fois sur le thread UI (`app/api.rs:438-464`). [partiel : cycles de `$ref` coupés ; pas de mémoïsation, fichier parsé 3 fois]
- **Correctif :** suivre les `$ref` déjà visités et rendre `null`, plafonner le nombre total de nœuds, importer en tâche de fond.

**P6. `src/api/store.rs`, `src/app.rs:1122` — faible.** Les appels D-Bus synchrones au trousseau se font au démarrage, à chaque `save_environments` (y compris après chaque extraction) et maintenant à chaque `save_collection` et au chargement (jusqu'à trois entrées par requête, pour les secrets d'Auth). Un trousseau verrouillé affiche une invite et bloque l'interface. [partiel : cache des secrets connus ; `api::State::load()` reste synchrone au démarrage]

### Fonctionnalités manquantes (par priorité)

1. **multipart/form-data :** `curl -F` est refusé (`curl.rs:247`) et les fichiers de `formdata` Postman sont supprimés (`postman.rs:160-181`).
2. **Pot de cookies** entre requêtes et pendant un Run : pas de feature `cookies`, un nouveau client par requête (`http.rs:75`). Une connexion par session puis un appel authentifié échoue.
3. **TLS d'entreprise :** AC personnalisée, certificat client (mTLS), réglage de proxy.
4. **Annulation et progression** d'un « Run collection » (`checks.rs:333`).
5. **Héritage d'auth et de variables par collection ou dossier :** actuellement aplati à l'import. Variables dynamiques `$guid` / `$timestamp`.
6. **Preview Markdown :** liens externes non cliquables (`markdown.rs:1061-1068`), images affichées en texte. Le comportement est sûr, mais limité.

### Dépendances & build

**D1. `release.yml` (build-linux) — faible à moyenne.** La compilation sur `ubuntu-latest` fixe le glibc minimal du `.deb` (`depends = "$auto"`) [plausible : impossible à installer sur Debian 12 / Ubuntu 22.04]. (Tests requis, `--locked` et toolchain épinglé : faits.)
- **Correctif :** runner Ubuntu fixe et ancien, ou build dans un conteneur Debian ancien.

**D2. `Cargo.toml:111-115` + `vendor/` — moyenne.**
- Le commentaire ne documente que le patch `iced_winit` (Cmd+Q, `lib.rs:81`, plus le jeton d'activation `conversion.rs:145`).
- `vendor/iced_graphics` est aussi patché : API `set_hidden_lines` pour le repli de code (`text/editor.rs:27-60,697-770`), utilisée par `src/code_editor.rs:1380`. Ce patch n'est signalé nulle part.
- L'historique git est écrasé (un seul commit) : aucun diff disponible. Le diff est reconstructible depuis `.cargo_vcs_info.json` (sha `3997291f`).
- **Effet sur la montée d'iced :** en 0.15, `[patch]` est ignoré avec un simple avertissement. La protection Cmd+Q disparaît silencieusement, et le repli casse la compilation.
- **Correctif :** versionner les `.patch` et un `vendor/PATCHES.md`, et ajouter un test qui échoue si le patch n'est plus appliqué.

**D3. `Cargo.toml:64` — faible à moyenne.** `serde_yaml 0.9.34+deprecated` (non maintenu, dépend de `unsafe-libyaml`) n'est utilisé qu'à `openapi.rs:16`.
- **Correctif :** `serde_norway` ou `serde_yaml_ng`, en évitant `serde_yml`.

**D4. `Cargo.toml:57-66` — faible à moyenne.** `postgres`, `tokio-postgres-rustls`, `rustls`, `rustls-native-certs`, `mysql` et `rusqlite` (`bundled`, du C) ne servent qu'aux binaires `drivers/` (rusqlite seulement en test côté `src/`). Ils sont pourtant compilés pour chaque build de l'éditeur.
- **Correctif :** un workspace avec une crate par pilote.

**D5. Divers — faible.**
- `once_cell` est remplaçable par `std::sync::LazyLock` (MSRV 1.90).
- Le `.deb` ne déclare pas `Recommends: pkexec`, pourtant requis par `updater.rs:292`.
- Le snap force le rendu logiciel (`snapcraft.yaml:67-73` : `LIBGL_ALWAYS_SOFTWARE`, llvmpipe) [plausible : lenteur en HiDPI]. `WINIT_UNIX_BACKEND` est probablement ignoré par winit 0.30 [plausible]. Les `plugs` sont sans effet en confinement `classic`.
- Pas de `postrm` pour rafraîchir le cache d'icônes.

---

**Fichiers lus en entier :** `src/updater.rs`, `src/ipc.rs`, `src/lsp_install.rs`, `src/session.rs`, `src/fsutil.rs`, `src/database/mod.rs` (l. 780-1000), `src/database/secrets.rs`, `install.sh`, `uninstall.sh`, `.github/workflows/{ci,release,pages}.yml`, `Cargo.toml`, `debian/postinst`, `snap/snapcraft.yaml`, `src/api/{mod,http,model,view,store,curl,postman,openapi,checks,testing}.rs`, `src/app/api.rs`, `src/preview/{mod,json,markdown}.rs`, `src/preferences/mod.rs`, `src/theme.rs`. J'ai aussi lu les extraits utiles de `src/main.rs`, `src/app.rs`, `src/lsp.rs` et `vendor/iced_{winit,graphics}`.
