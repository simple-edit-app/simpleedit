# Audit de SimpleEdit 1.26.2

> Commit `36a7f58` (branche `main`), audit du 12 novembre 2026
> Périmètre : ~130 000 lignes de Rust en 126 fichiers (`src/`, `drivers/`), dont ~95 000 hors modules de test.

## Sommaire

1. [Méthode, couverture et limites](#1-méthode-couverture-et-limites)
2. [Verdict](#2-verdict)
3. [Priorités](#3-priorités)
4. [Perte ou corruption de données](#4-perte-ou-corruption-de-données)
5. [Bugs fonctionnels](#5-bugs-fonctionnels)
6. [Sécurité](#6-sécurité)
7. [Performances : fenêtre qui se fige ou accroche](#7-performances--fenêtre-qui-se-fige-ou-accroche)
8. [Performances : coût permanent](#8-performances--coût-permanent)
9. [Vérifié, sans problème](#9-vérifié-sans-problème)
10. [Déjà corrigé en 1.26.1 et 1.26.2](#10-déjà-corrigé-en-1261-et-1262)

---

## 1. Méthode, couverture et limites

**Vérifications automatiques** (sur `36a7f58`) :
- `cargo fmt --check` : propre.
- `cargo clippy --all-targets -- -D warnings` : 0 avertissement.
- `cargo test --workspace` : 1 216 + 118 tests passés, 0 échec, 21 ignorés (tests « live » et captures).

**Point de départ.** Deux bugs vus en usage réel le jour même :
- les menus qui se fermaient seuls ;
- la fenêtre figée à l'ouverture d'un gros projet.

Les deux sont corrigés (§ 10). Chacun révélait une *classe* de bug, cherchée ensuite dans tout le code.

**Couverture.**

| Niveau | Fichiers |
|---|---|
| **Lus en entier** (hors tests) | `main.rs`, `ipc.rs`, `app.rs` (9 253 lignes), `app/file_io.rs`, `app/remote_files.rs`, `app/compare.rs`, `app/project_ops.rs`, `app/tasks.rs`, `app/spelling.rs`, `app/split.rs`, `app/bookmark_panel.rs`, `app/git_file.rs`, `app/lint.rs`, `app/results.rs`, `app/api.rs`, `tabs.rs`, `find_replace.rs`, `watch.rs`, `menu_bar.rs`, `fsutil.rs` (écriture atomique) |
| **Lus par parties** (fonctions à risque) | `app/completion.rs`, `app/local_history.rs`, `app/views.rs` (session), `app/finder.rs` (recherche, remplacement, aperçu), `app/everywhere.rs` (chargement, classement), `app/db_tools.rs` (import), `code_editor.rs` (`Content`, multi-curseurs, `select_bytes`), `editor/mod.rs`, `projects.rs`, `finder.rs`, `updater.rs`, `git/tools.rs`, `git_panel/log.rs` (reset), `database/protocol.rs` (`risk_of`), `api/store.rs`, `terminal.rs`, `lsp.rs` (arrêt des serveurs) |
| **Balayés par classe de bug** | tout le reste (git, git_panel, database, drivers, api, lsp, lint, preview, sidebar, preferences…) |

Les classes cherchées dans **tout** le code :
1. Messages qui arrivent seuls (minuteries, observateurs, réponses asynchrones) et ferment l'interface.
2. Sorties de programmes lues en anglais.
3. E/S ou processus synchrones sur le fil principal.
4. Commandes externes sans limite de temps, ou exposées à l'interblocage stdin/stdout.
5. Paniques sur données inattendues (`unwrap`, `expect`, découpage de chaîne à un octet non vérifié).
6. Écritures non atomiques de fichiers de l'utilisateur.
7. Travail refait à chaque image (`view()`).
8. Actions destructrices sans confirmation.

**Limite.** Les fichiers balayés n'ont pas été relus ligne à ligne. Ce sont surtout :
- la grille et les pilotes de bases de données ;
- le client API (hors historique) ;
- les serveurs de langage (hors arrêt) ;
- l'aperçu Markdown ;
- l'émulation du terminal ;
- le panneau Git (hors reset et récupération de révision).

Ils ont été lus en entier par l'audit de la 1.25.0. Un bug de logique propre à l'un d'eux, hors des 8 classes, a pu échapper.

**Marques**

| Marque | Signification |
|---|---|
| ✅ **test** | Reproduit par un test unitaire temporaire, lancé sur `36a7f58` puis retiré. |
| ✅ **mesuré** | Mesuré sur l'app réelle ou sur un vrai projet. |
| 🔎 **code** | Chemin de code relu et confirmé. |
| **[plausible]** | Mécanisme établi par le code, effet non observé. |

**Sévérité.**
- **Haute** : perte de données, ou usage courant bloqué.
- **Moyenne** : gêne visible ou risque réel.
- **Basse** : cas rare ou coût faible.

---

## 2. Verdict

Le code est en bon état : lint propre, tests verts, corrections de l'audit 1.25.0 en place. Les points solides sont listés au § 9 (mise à jour signée, limites de temps, écritures atomiques).

Les problèmes restants se rangent en quatre familles :

1. **Perte de données sur des chemins rares mais réels.**
   - Fermer l'app jette les modifications en attente d'une grille BDD ou d'une fusion Git, sans question (D1).
   - « Récupérer depuis une révision » écrase un fichier sans confirmation (D2).
   - Les fichiers distants ne sont plus renvoyés au serveur après un redémarrage (D3).
   - Les fins de ligne mixtes sont réécrites en silence (D4).
2. **La liste des messages qui laissent un menu ouvert se tient à la main.** Cinq messages manquent encore (F1). Correctif de fond : inverser la logique, seules les actions de l'utilisateur ferment un menu.
3. **Du travail synchrone sur le fil principal qui grandit avec le projet.** C'était la cause du gel corrigé en 1.26.2, et il en reste une quinzaine de cas (§ 7).
4. **Des formats non UTF-8 ou non anglais mal gérés** : import CSV Windows-1252 impossible (F4), erreurs de compilateur en français ignorées (F7).

---

## 3. Priorités

| # | Constat | Sévérité |
|---|---|---|
| 1 | D1 — Quitter jette les modifications BDD et de fusion en attente | Haute |
| 2 | D2 — « Récupérer depuis une révision » sans confirmation | Haute |
| 3 | F1 — 5 messages ferment encore les menus (build, auto-save, tâche…) | Haute |
| 4 | D3 — Fichiers distants plus renvoyés après redémarrage | Moyenne |
| 5 | S1 — Jetons API en clair dans l'historique | Moyenne |
| 6 | F2 — Alerte « sans WHERE » trompée par une sous-requête ou un `WITH` | Moyenne |
| 7 | F4 — Import CSV Windows-1252 impossible | Moyenne |
| 8 | P1 à P6 — gels du fil principal | Moyenne |

---

## 4. Perte ou corruption de données

### D1. Quitter jette les modifications en attente d'une grille BDD et d'une fusion — **Haute** 🔎 code

- **Où :**
  - `src/app.rs:4470-4495` (`Message::Quit`) ;
  - `src/app/views.rs:1071-1159` (`build_session`).
- **Scénario :**
  1. Modifier des cellules d'une table, sans « Commit », ou résoudre à moitié un conflit dans un onglet `merge://`.
  2. Fermer la fenêtre principale.
  3. La session garde les fichiers et les requêtes API modifiés, mais ni les modifications en attente de la grille (`db_tabs[..].pending`, pourtant comptées par `is_path_dirty`), ni l'état de la fusion.
  4. L'app se ferme sans question, et ce travail est perdu. La seconde fenêtre, elle, demande confirmation.
- **Correctif :** avant de quitter, si un onglet BDD a des modifications en attente ou si un onglet de fusion est modifié, ouvrir `ConfirmQuit`.

### D2. « Récupérer depuis une révision » écrase le fichier sans confirmation ni retour — **Haute** 🔎 code

- **Où :** `src/app/git_file.rs:581-588` → `git/tools.rs:21-29`.
- **Scénario :**
  1. Menu Git d'un fichier → « Récupérer depuis… ».
  2. Un clic dans la liste lance `git restore --source=<rev> --staged --worktree -- <fichier>`.
  3. Les modifications non commitées et ce qui était indexé sont perdus, sans confirmation.
  4. Rien n'est gardé : pas de `remember_file`, donc Ctrl+Z dans un diff ne le défait pas, et pas de copie dans l'historique local.
- **Correctif :** confirmation quand le fichier a des changements, puis `remember_file` et `history_record` avant d'écrire.

### D3. Les fichiers distants (SSH) ne sont plus renvoyés après un redémarrage — **Moyenne** 🔎 code

- **Où :** `src/app/remote_files.rs:166`, `src/app.rs:1007`.
- **Scénario :**
  1. Le lien « copie locale ↔ fichier distant » (`remote_files`) ne vit qu'en mémoire.
  2. Après un redémarrage, l'onglet de la copie revient, puisque c'est un chemin local.
  3. Enregistrer n'envoie plus rien au serveur, sans avertissement : on croit avoir mis à jour le fichier distant.
- **Correctif :** sauver `remote_files` dans la session (ou le déduire du chemin sous `…/simpleedit/remote/<hôte>/`).

### D4. Fins de ligne mixtes et `\r` isolés réécrits en silence — **Moyenne** 🔎 code

- **Où :** `src/tabs.rs:208-250`.
- **Scénario :**
  1. À l'ouverture, toutes les fins de ligne deviennent `\n`, et un `\r` isolé devient un saut de ligne.
  2. À l'enregistrement, toutes les lignes prennent la fin dominante.
  3. Un fichier aux fins mélangées (quelques lignes CRLF dans un fichier LF) est donc entièrement réécrit dès qu'on change une lettre, et Git montre un diff sur tout le fichier.
  4. Un `\r` voulu (dans une chaîne, un log) devient un vrai saut de ligne.
- **Correctif :**
  - signaler « Fins mixtes » dans la barre d'état et ne normaliser que sur demande ;
  - à défaut, conserver la fin d'origine des lignes non touchées.

### D5. Écritures non atomiques de fichiers de l'utilisateur — **Basse** 🔎 code

- **Où :**
  - `src/git/mod.rs:493`, `850`, `1053` (résolution de conflit, annulation d'un bloc) ;
  - `src/app/spelling.rs:275` (dictionnaire personnel) ;
  - `src/app/local_history.rs:375` (recréation d'un fichier supprimé).
- **Scénario :** un plantage ou un disque plein en pleine écriture laisse un fichier tronqué. Pour le dictionnaire, l'erreur est aussi ignorée : le mot semble ajouté mais ne l'est pas.
- **Correctif :** `fsutil::write_atomic` partout, avec un message en cas d'erreur.

### D6. Renommer le fichier ouvert efface son historique d'annulation — **Basse** 🔎 code

- **Où :** `src/app/project_ops.rs:245-246` → `src/app.rs:7801-7808`.
- **Scénario :** renommer ou déplacer le fichier en cours d'édition ferme puis rouvre l'onglet. `close_path` vide `undo_stack`/`redo_stack`, la position de défilement et les plis.
- **Correctif :** déplacer l'onglet courant comme les autres, en renommant la clé.

---

## 5. Bugs fonctionnels

### F1. Cinq messages ferment encore les menus tout seuls — **Haute** ✅ test (3) · 🔎 code (2)

- **Où :** `src/app.rs:2362` (`is_background`), `src/app.rs:2433` (`closes_menu`).
- **Messages en cause :**

| Message | Quand | Preuve |
|---|---|---|
| `FsChanged` | un build ou un serveur de dev écrit dans le projet (jusqu'à 5×/s) | ✅ test |
| `FileSaved` | l'enregistrement automatique, 0,7 à 2 s après la frappe | ✅ test |
| `FileReloaded` / `LargeRefreshed` | le fichier ouvert est réécrit par un autre programme | ✅ test |
| `TaskEvent` | chaque paquet de sortie d'une tâche en cours | 🔎 code |
| `SymbolHoverLoaded` d'avant l'ouverture | réponse LSP tardive | corrigé en 1.26.1 |

- **Correctif immédiat :** les ajouter à `is_background`.
- **Correctif de fond :** remplacer la liste noire par une liste blanche, où seuls les messages d'action utilisateur ferment un menu.

### F2. L'alerte « UPDATE/DELETE sans WHERE » se laisse tromper — **Moyenne** 🔎 code

- **Où :** `src/database/protocol.rs:996-1022` (`risk_of`).
- **Scénario :**
  - `UPDATE t SET a = (SELECT b FROM u WHERE u.id = 1)` modifie toute la table sans alerte, car le `WHERE` est celui de la sous-requête.
  - `WITH x AS (…) DELETE FROM t` n'est pas examiné, car le premier mot est `WITH`.
  - Sur une base non marquée « production », ces requêtes partent sans aucune question.
- **Correctif :** ne compter que le `WHERE` de profondeur de parenthèses 0, et regarder le verbe qui suit un `WITH`.

### F3. L'éditeur perd probablement le focus après une commande de menu — **Moyenne** [plausible]

- **Où :** `src/app.rs:1931-2042`, `2068-2205`.
- **Mécanisme :**
  1. Ouvrir un menu (comme la liste des onglets, le menu Git, une fenêtre BDD ou le toast de conflits) change la forme de l'arbre de widgets : `base` devient `stack![base, outside]`.
  2. iced recrée alors l'état de tout ce qui est dessous.
  3. Le code le sait pour les cartes flottantes, qu'il garde toujours présentes, vides si fermées (`app.rs:1994-1997`), mais pas pour les menus.
- **Effet attendu :** après Édition > Annuler, taper ne va plus dans l'éditeur ; les défilements non pilotés reviennent en haut ; le toast de conflits qui apparaît pendant la frappe retire le focus.
- **Correctif :** couches toujours présentes, comme pour les cartes.

### F4. L'import CSV/JSON n'accepte que l'UTF-8 — **Moyenne** 🔎 code

- **Où :** `src/app/db_tools.rs:635-648`, `drivers/common/mod.rs:761`.
- **Scénario :**
  1. Un CSV exporté par Excel en français est presque toujours en Windows-1252.
  2. L'aperçu affiche `�` à la place des accents (`from_utf8_lossy`).
  3. Le pilote fait `read_to_string`, qui échoue avec « stream did not contain valid UTF-8 » (en anglais).
  4. Le fichier ne peut pas être importé. Il est en plus chargé en entier en mémoire.
- **Correctif :** détecter l'encodage (`Encoding::detect` existe déjà), le proposer dans la fenêtre d'import, le passer au pilote, et lire en flux.

### F5. Un dossier tout juste créé n'est surveillé qu'au bout de 5 s — **Basse** 🔎 code

- **Où :** `src/watch.rs:195` (`watch_created`), appelé seulement par `sync_watch`, tous les 5 ticks (`src/app.rs:4759`).
- **Scénario :** après `mkdir nouveau && touch nouveau/a.txt`, la création de `a.txt` n'est pas entendue ; l'arbre ne la montre qu'au rafraîchissement suivant (30 s au plus).
- **Correctif :** appeler `watch_created()` sur `FsChanged`.

### F6. « Dupliquer » un dossier laisse une copie partielle en cas d'échec — **Basse** 🔎 code

- **Où :** `src/app/project_ops.rs:37`, `177`.
- **Scénario :** disque plein ou droits en cours de copie : la copie à moitié faite reste. Le gel de la fenêtre pendant la copie est traité en P1.
- **Correctif :** supprimer la cible sur erreur.

### F7. Erreurs de compilateur en français ignorées par le panneau Tâches — **Basse** 🔎 code

- **Où :** `src/app/tasks.rs:60-69`.
- **Scénario :** seuls `error` et `warning` sont reconnus. gcc, clang, make, javac en français écrivent `erreur :` / `attention :`, donc aucun problème ne remonte. C'est la même classe que le bug de `git clean`.
- **Correctif :** lancer les tâches avec `LC_MESSAGES=C`, ou reconnaître aussi les mots traduits.

### F8. Autres bugs fonctionnels — **Basse** 🔎 code

| Constat | Où | Correctif |
|---|---|---|
| Les fichiers UTF-16 sans BOM sont refusés comme « binaires ». | `app/file_io.rs:45-48` | Détecter un UTF-16 probable (un octet nul sur deux). |
| Les fichiers de plus de 64 Mo s'ouvrent sans test « binaire », décodés en `from_utf8_lossy` (Latin-1 → `�`). | `app/file_io.rs:153-167` | Appliquer `is_binary` et l'encodage détecté. |
| « Comparer avec le disque » décode avec l'encodage détecté, pas celui de l'onglet (différences fantômes). | `app/compare.rs:65` | Utiliser `self.format.encoding`. |
| Les noms de fichiers non UTF-8 manquent dans Ctrl+P. | `finder.rs:108`, `124` | `OsStr::from_bytes`. |
| Le déplacement vers un autre disque échoue (« Invalid cross-device link »). | `app/project_ops.rs:213` | Copie puis suppression sur EXDEV. |
| Le panneau des signets montre le texte du disque, pas celui de l'onglet modifié. | `app/bookmark_panel.rs:86-106` | Prendre le texte du `file_cache`. |
| L'onglet d'aperçu remplacé n'envoie pas `did_close` au serveur de langage. | `app.rs:7984` (`let _ = self.close_path(old)`) | Fusionner la tâche rendue. |
| « Lancer la collection » pendant un lancement affiche « Lancement… » puis ne fait rien. | `app/api.rs:353-377` | Tester `api.run` avant le statut. |
| `autosaving` est un booléen pour un lot : statut « Fichier enregistré » sur un auto-save, auto-save qui repart pendant un lot. [plausible] | `app.rs:2782`, `7096-7120` | Un compteur. |
| « Nouvelle fenêtre » laisse un processus zombie à chaque fermeture. | `app.rs:3746-3753` | `wait()` du fils dans un fil. |
| Fermer un terminal tue le shell sans `wait` ni son groupe : un serveur lancé dedans peut survivre, orphelin. [plausible] | `terminal.rs:360-366` | `killpg` comme `tasks.rs`, puis `wait`. |

---

## 6. Sécurité

### S1. Jetons et mots de passe en clair dans l'historique des requêtes API — **Moyenne** 🔎 code

- **Où :** `src/api/store.rs:342-360`, `118-124`.
- **Problème :** l'historique (`history.json`) n'efface que les champs de l'onglet Auth.
- **Restent écrits en clair à chaque envoi :**
  - un en-tête `Authorization: Bearer …`, `Cookie` ou `X-Api-Key` tapé, ou importé d'une commande curl, qui met tout dans les en-têtes ;
  - un paramètre `?api_key=` ou `?token=`.
- **Correctif :** masquer aussi les en-têtes et paramètres sensibles (`Authorization`, `Cookie`, `Proxy-Authorization`, `*key*`, `*token*`, `*secret*`, `*password*`), sauf s'ils ne contiennent que des `{{variables}}`.

### S2. Le trousseau du système est sollicité à chaque touche — **Moyenne** 🔎 code

- **Où :** `src/app/api.rs:278-289`, `src/api/store.rs:308-310`.
- **Problème :**
  - Renommer la clé d'une variable secrète appelle `forget_secret` (D-Bus/Keychain, synchrone, fil principal) à chaque touche : la saisie accroche.
  - Si l'app s'arrête avant la fermeture de la fenêtre, le secret est déjà effacé sous l'ancien nom.
  - `save_environments` réécrit aussi tous les secrets à chaque choix d'environnement.
- **Correctif :** ne toucher au trousseau qu'à l'enregistrement, en tâche de fond.

---

## 7. Performances : fenêtre qui se fige ou accroche

Toutes ces opérations tournent sur le fil principal et grandissent avec la taille des données.

| # | Opération | Où | Effet | Correctif |
|---|---|---|---|---|
| P1 | « Dupliquer » un dossier (copie récursive, sans limite) | `app/project_ops.rs:177` | gel le temps de la copie (`node_modules` : des minutes) | `spawn_blocking` + message |
| P2 | Renommer un symbole, « Tout remplacer » dans les fichiers, revenir à une étiquette de l'historique | `app.rs:8563-8624`, `app/finder.rs:1010-1040` | lecture + écriture avec fsync de chaque fichier non ouvert : secondes sur des centaines de fichiers | écritures en tâche de fond |
| P3 | « Tout remplacer » : regroupement par fichier par recherche linéaire | `app/finder.rs:938-944` | O(résultats × fichiers) : ~500 M comparaisons pour 100 000 résultats / 5 000 fichiers ; chaque fichier lu 3 fois | `HashMap`, garder l'encodage de la 1ʳᵉ lecture |
| P4 | Chargement du dictionnaire d'orthographe | `app/spelling.rs:79-109` | gel à la 1ʳᵉ ouverture de fichier et à chaque changement de langue | `spawn_blocking` |
| P5 | Rafraîchissement de l'arbre : `read_dir` + `git check-ignore` synchrone par dossier déplié modifié | `projects.rs:311-335`, `180-215` | accroches pendant un build (jusqu'à 5×/s) ; `stat` qui bloque sur un partage réseau [plausible] | `load` en tâche de fond |
| P6 | Fichiers brouillons réécrits chaque seconde avec fsync pendant la frappe | `app.rs:7857-7880` | saccade toutes les secondes sur disque lent | même chemin que l'auto-save |
| P7 | `config.save()` (fsync) à chaque cran de Ctrl+molette, changement de thème, de contraste ; signets toutes les 3 s s'ils ont bougé | `app.rs:4094-4101`, `4365-4379`, `app/views.rs:1189` | saccades | marquer sale, écrire en fond |
| P8 | Lecture synchrone de fichiers jusqu'à 64 Mo : changer d'encodage, rétablir, « Comparer avec le disque », aperçu de « Chercher dans les fichiers », ouverture du panneau des signets, choix d'un JSON à importer | `app.rs:3995`, `4036` ; `app/compare.rs:65` ; `app/finder.rs:712-755` ; `app/bookmark_panel.rs:86` ; `app/db_tools.rs:180` | gel le temps de la lecture | `spawn_blocking` |
| P9 | Classement de tous les fichiers à chaque touche dans « Chercher partout » | `app/everywhere.rs:659-700` | 1,3 à 1,6 s pour 200 000 chemins en debug (test du dépôt) ; bien moins en release [plausible] | classement en fond avec génération |
| P10 | Vérification orthographique de tout le fichier (≤ 2 000 lignes) à chaque touche | `app/spelling.rs:145-159` | coût par touche proportionnel au fichier | lignes modifiées seulement, ou toujours après la pause de 250 ms |
| P11 | Suppression d'un dossier : jusqu'à 2 000 fichiers lus pour l'historique local sur un fil asynchrone | `app/project_ops.rs:293` | bloque un fil tokio | dans le même `spawn_blocking` que la suppression |

---

## 8. Performances : coût permanent

| # | Coût | Où | Correctif |
|---|---|---|---|
| C1 | Minuterie IPC de 300 ms : `update` + `view()` complets 3,3×/s, même au repos | `app.rs:1614` | `Subscription` alimentée par le socket |
| C2 | Chaque message, même passif (IPC, `TerminalTick` 30×/s, `AutoSave`), refait `sync_split`, `after_message`, `lint_tick`, `refresh_preview`, `refresh_bookmark_panel`… | `app.rs:1401-1482` | court-circuiter pour les messages passifs |
| C3 | L'arbre des projets clone toutes ses lignes visibles à chaque image (`projects.rows()`), puis n'en dessine qu'une partie | `sidebar/mod.rs:573`, `projects.rs:526` | lignes en cache |
| C4 | Panneau Résultats : comptes par fichier et mise en page de *tous* les résultats à chaque image | `app/results.rs:525-555` | mise en page en cache |
| C5 | `dirty_files()`, `marked_paths()` (HashSet) et `problem_counts()` (clone de tous les diagnostics) à chaque image | `app.rs:1885-1896`, `9149` ; `app/lint.rs:327-337` | tenus à jour dans `update` |
| C6 | Pour un fichier < 3 000 lignes, chaque frappe recopie le buffer et recalcule le diff avec HEAD | `app.rs:6966`, `7015-7031` | regrouper (~150 ms) |
| C7 | Chaque survol d'un mot copie tout le buffer pour la requête LSP | `app.rs:3808-3830` | réutiliser le texte déjà synchronisé |
| C8 | `completion_prepare()` remonte l'arborescence (`project_root`, plusieurs `stat`) à chaque touche | `app/completion.rs:235-251` | mémoriser par fichier |
| C9 | Chaque étape d'annulation est une copie complète du texte (fusion seulement au-delà de 2 Mo) | `app.rs:6512-6531` | historique par différences |

---

## 9. Vérifié, sans problème

| Zone | Vérifié |
|---|---|
| Mise à jour (`updater.rs`) | `SHA256SUMS` signé Ed25519 (`verify_strict`), empreinte revérifiée avant installation, téléchargements limités. |
| Commandes externes | Formateur, linters, serveurs de langage, installation, SSH : limite de temps et `kill_on_drop` ; stdin écrit depuis une tâche à part, pas d'interblocage. |
| Sorties en langue étrangère | `[rejected]`, `fetch first`, `Binary files … differ` (Git 2.53) et les messages d'OpenSSH ne sont pas traduits ; seul `git clean` l'était, corrigé en 1.26.1. |
| Reset Git | Confirmation, avec avertissement en rouge pour `--hard`. |
| Liste des fichiers (Ctrl+P) | En `spawn_blocking`, via `git ls-files` (pas de `node_modules`). |
| Recherche dans les fichiers | En `spawn_blocking`, annulable, avec génération. |
| Historique local | Écritures et élagage en arrière-plan. |
| Config, session, signets | `fsutil::write_atomic` (fichier temporaire, fsync, renommage). |
| Session | Écrite en tâche de fond ; garde les buffers modifiés et les requêtes API changées (sans leurs secrets). |
| Découpages de chaînes | Colonnes issues de positions de curseur (frontières de caractère) ou `floor_boundary` / `is_char_boundary` ; pas de panique trouvée. |
| `unwrap`/`expect` hors tests | Tous gardés par le code autour. |
| Terminal | Rafraîchissement limité à 30 images/s ; collage nettoyé des séquences d'échappement. |
| Vue des différences | Mise en page en cache. |

---

## 10. Déjà corrigé en 1.26.1 et 1.26.2

| Version | Correction |
|---|---|
| 1.26.1 | La minuterie IPC de 300 ms fermait les menus : « Vérifier les mises à jour » était hors d'atteinte. |
| 1.26.1 | Le pointeur atteignait l'éditeur sous le menu, et le survol d'un mot le fermait. |
| 1.26.1 | `git clean` n'affichait rien quand Git parlait français. |
| 1.26.1 | Le test de vitesse de la recherche rapide échouait sur machine chargée. |
| 1.26.2 | Gel d'environ 40 s à l'ouverture d'un gros projet : sous Linux, notify surveillait tout l'arbre sur le fil principal (26 918 dossiers mesurés). Seuls les dossiers utiles sont surveillés désormais, 1 375 placés en 74 ms (✅ mesuré). |
