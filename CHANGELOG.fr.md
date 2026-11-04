# Journal des modifications

## [1.8.0] - 2026-11-04

### Ajouté
- Lint : les problèmes trouvés par les linters et les serveurs de langage s'affichent dans l'éditeur, avec une carte au survol qui donne le message, la règle et les corrections. Désactivé au départ : on l'active pour chaque langage dans Préférences > Lint (on peut aussi l'activer depuis le clic droit de l'éditeur).
- Les outils sont trouvés tout seuls, jamais installés : ceux du projet (`node_modules/.bin`, `.venv`, `vendor/bin`), puis ceux du système ; on peut choisir un chemin pour chaque outil. ESLint, TypeScript (serveur de langage), Ruff, flake8, pycodestyle, pylint, mypy, Clippy, go vet, golangci-lint, cppcheck, ShellCheck, Stylelint, yamllint, markdownlint, RuboCop, php, luacheck, ktlint, SwiftLint, SQLFluff, HLint, xmllint, JSON, et les serveurs de langage de Rust, Go, C/C++, Java, C#, Ruby, PHP, Kotlin, Swift, Dart, Lua, Haskell et d'autres.
- Corrections : un problème depuis sa carte, ou tous les problèmes de la même source (ESLint, TypeScript…) du fichier ; « Corriger les problèmes du fichier » depuis le menu Édition, la palette, le clic droit de l'éditeur et d'un fichier de la liste. Par défaut, seules les erreurs sont corrigées (les avertissements, ou tout, dans les réglages) ; les corrections peuvent aussi être appliquées à l'enregistrement.
- Chaque gravité a son marquage (trait ondulé, trait droit, surlignage, ou trait ondulé et surlignage) et sa couleur. Les problèmes apparaissent aussi dans la minimap et dans la barre d'état (`✖ 2 ⚠ 3`, un clic va au suivant) ; problème suivant et précédent dans le menu Édition et la palette.
- Quand le lint tourne : pendant la frappe, à l'enregistrement ou seulement sur demande, et une première analyse à l'ouverture d'un fichier (une option). La barre d'état dit quand aucun outil n'a été trouvé, et ce dont un outil en échec s'est plaint.
- Les Préférences sont dans le menu Fichier.

### Corrigé
- L'éditeur ne perd plus son focus ni son curseur quand une carte flottante s'ouvre ou se ferme.

## [1.7.0] - 2026-11-04

### Ajouté
- Éditeur divisé : l'éditeur peut être divisé autant de fois que l'on veut, côte à côte ou l'un au-dessus de l'autre, et de nouveau dans n'importe quel panneau. On le trouve dans le menu Affichage, la palette de commandes, le clic droit de l'éditeur et, pour montrer un fichier dans un nouveau panneau, le clic droit de la liste des fichiers (Ouvrir à droite, Ouvrir en bas). Chaque panneau a un en-tête avec son fichier et une croix pour le fermer. Un clic dans un panneau lui donne le focus ; la molette le fait défiler sans. Les panneaux sur un même fichier gardent leur curseur et leur défilement, et le texte suit.

## [1.6.0] - 2026-11-04

### Ajouté
- La liste des fichiers se redimensionne en tirant son bord (sa largeur est retenue) et affiche le chemin complet dans une infobulle. Clic droit, Supprimer, supprime vraiment un fichier, avec une confirmation et une case pour le retirer aussi de Git quand il est suivi.
- Les modifications d'un fichier versionné sont aussi marquées dans la minimap par une bande, comme les signets. Dans la marge, le curseur est une flèche, et une main sur les chevrons de repli, les modifications et les signets. Le blame affiche une carte à la IntelliJ après un court délai, et « version précédente de cette ligne » ouvre le fichier tel qu'il était avant ce commit.
- Raccourcis clavier revus d'après IntelliJ (les raccourcis personnalisés sont conservés), et Ctrl+Maj+B pour la liste des branches.
- Panneau Git : Fetch (et fetch automatique réglé dans les Préférences) avec une alerte de nouveaux commits ; Pull et Merge en boutons scindés dont les modes (pull, pull --rebase, fast-forward seul, merge, squash...) se règlent dans les Préférences ; les nouvelles branches et tags se nomment dans une fenêtre ; les tags se poussent et se suppriment sur origin ; annuler le dernier commit ; mettre un seul fichier de côté ; ajouter un fichier à .gitignore.
- Diffs : indexer ou désindexer un bloc ou les lignes sélectionnées, mots modifiés surlignés dans une ligne, un bouton pour ignorer les espaces, un bouton pour changer le contexte (3, 10, 30 lignes, fichier entier), et la coloration syntaxique.
- Log : un menu clic droit pour créer une branche ou un tag ici, checkout, reset (soft, mixed, hard) et comparer ; une recherche par message, auteur, texte ajouté ou retiré, fichier ou date ; et un rebase interactif (réordonner, renommer, éditer, squash, fixup, supprimer).
- Un sélecteur de branche dans la barre d'état, avec filtre et création de branche.
- Les panneaux du bas (Git, terminal, fichiers scratch, signets) se redimensionnent en tirant leur bord haut. Les boutons sous la liste des fichiers suivent la largeur de la liste, qui ne peut pas être plus étroite que ce qu'ils demandent.
- Un fichier scratch créé sans nom s'appelle `scratch1`, `scratch2`...

### Corrigé
- Une erreur Git s'affiche dans la barre d'état quand le panneau Git est fermé.

## [1.5.0] - 2026-11-03

### Ajouté
- Panneau Git (bouton à l'icône de branche sous la liste des fichiers, ou son raccourci) avec quatre onglets. **Commit** : fichiers indexés et modifiés, message de commit, Modifier le dernier commit, Commit et Push, et le diff du fichier ouvert. **Stash** : enregistrer, appliquer, retirer et supprimer. **Log** : le graphe des commits par branche avec les fichiers et le message d'un commit, copier le hash, cherry-pick, revert, supprimer, regrouper (squash) et renommer le message. **Branches** : branches locales et distantes et tags, pour créer, basculer, fusionner, rebaser, pousser, forcer le push (avec protection) et supprimer.
- Les diffs s'ouvrent par défaut côte à côte, adaptés à la largeur de la fenêtre, ou en vue unifiée (un bouton avec infobulle bascule). On peut sélectionner le texte au caractère (un double-clic prend un mot) et le copier (Ctrl+C, Ctrl+A, clic droit). Des flèches remettent un bloc de changements, y compris pour les fichiers indexés. Un diff peut s'ouvrir dans un onglet de l'éditeur, être comparé au fichier actuel, et se suivre avec Ctrl+Z.
- Un sous-menu Git sur un fichier (liste des fichiers et clic droit de l'éditeur) : indexer, commit de ce fichier, diff, comparer avec une branche, un tag ou une révision, et l'historique du fichier.
- Les lignes modifiées sont marquées dans la gouttière et à côté de la barre de défilement. Un clic sur une barre affiche une flèche unique qui remet le changement.
- Conflits (rebase, merge, cherry-pick, revert) : une alerte en haut les annonce avec un bouton pour les résoudre, et disparaît toute seule dès qu'ils sont résolus ou que l'opération est abandonnée. Chaque fichier en conflit s'ouvre en trois colonnes avec un résultat modifiable, la résolution d'un côté en un clic, la résolution automatique des conflits simples, annuler et rétablir.
- Blame dans la gouttière, et un bouton pour montrer le commit d'une ligne dans le log.
- Les fichiers s'enregistrent tout seuls un instant après la dernière modification (une option des Préférences).

### Corrigé
- Annuler un renommage indexé remet l'ancien nom ; une ligne supprimée qui commence par `-- ` reste dans le diff ; le dernier commit peut être supprimé ; renommer le dernier commit n'y ajoute plus ce qui est indexé ; l'historique d'un fichier se dessine sur une seule ligne.

## [1.4.0] - 2026-11-03

### Ajouté
- Panneau des marque-pages : un bouton sous la liste des fichiers ouvre un panneau avec chaque fichier marqué et, dessous, ses lignes marquées (`58: du code`). Un double-clic ouvre le fichier, ou le fichier à la ligne choisie. On ajoute un fichier depuis le clic droit de la liste des fichiers, une ligne depuis le clic droit de l'éditeur (le curseur va maintenant sur la ligne cliquée). Un clic droit dans le panneau supprime un marque-page.
- Les marque-pages s'affichent en ruban jaune dans la gouttière et en repère jaune dans l'aperçu à droite.

## [1.3.0] - 2026-11-03

### Ajouté
- Fichiers scratch : un bouton sous la liste des fichiers ouvre un panneau pour créer des notes et des essais hors de tout projet : un nom et une extension choisie dans une liste alphabétique (ou aucune). Ils s'enregistrent pendant la frappe, ne se ferment jamais, restent hors de la liste des fichiers et durent jusqu'à leur suppression (clic droit, Supprimer, avec confirmation). Leur dossier se change dans les Préférences, et les fichiers le suivent.

## [1.2.0] - 2026-11-02

### Ajouté
- Terminal intégré : un bouton `>_` sous la liste des fichiers ouvre un panneau de terminal en bas de la fenêtre, avec des onglets pour lancer plusieurs shells (« + » en ajoute un, « × » en ferme un, « — » masque le panneau). Il démarre dans le projet du fichier ouvert et gère les couleurs, le défilement à la molette, la sélection à la souris, le copier-coller (Ctrl+Maj+C / V, Cmd+C / V sur macOS) et les programmes plein écran. Alt+F12 (modifiable), le menu Affichage et la palette de commandes l'ouvrent et le ferment.

## [1.1.0] - 2026-11-02

### Ajouté
- Une petite croix à droite de chaque fichier de la barre latérale ferme son onglet aussitôt, sans confirmation, même avec des modifications non enregistrées.
- « Fermer les autres onglets » et « Fermer tous les onglets » dans le menu Fichier, la palette de commandes et le menu du clic droit sur un fichier de la barre latérale (là, « les autres » garde le fichier sur lequel vous avez cliqué). Une seule confirmation couvre tous les onglets modifiés.

### Corrigé
- Un clic n'importe où en dehors d'un menu ouvert le ferme, y compris sur les zones vides de la barre latérale, de la barre d'outils et de la barre d'état.

## [1.0.1] - 2026-11-02

### Corrigé
- La coloration syntaxique ne rend plus le reste du fichier vert après un texte entre accents graves contenant `${…}` en JavaScript, TypeScript, JSX et TSX, et un nom de balise JSX en fin de ligne (`<button` suivi de ses attributs) a la même couleur que sa balise fermante.
- JSX, TypeScript, TSX, Kotlin, Swift, SCSS, Less et TOML ont maintenant leur propre grammaire au lieu de celle d'un langage voisin, et les fichiers JSX ne finissent plus dans une fausse expression régulière.

## [1.0.0] - 2026-11-02

### Ajouté
- Première version de SimpleEdit : un éditeur de texte rapide, stable et multiplateforme pour macOS (Apple Silicon) et Linux (x86_64), écrit en Rust.
- Édition : coloration syntaxique pour plus de 30 langages, recherche et remplacement avec expressions régulières, opérations sur les lignes, pliage de code, marque-pages, aperçu Markdown, formatage du code par des outils externes, thèmes sombre et clair, français et anglais, et une session qui conserve le texte non enregistré entre deux lancements.
- Aller à la définition : Ctrl+clic (Cmd+clic sur macOS) sur une fonction, une variable, une classe, une interface… mène à sa définition, et sur une définition il liste les endroits où elle est utilisée. Il ne répond que s'il est sûr de lui : jamais de résultat faux.
- Retour / Avance (Alt+Gauche / Alt+Droite, boutons de la souris, menu et palette de commandes) parcourent les endroits d'où l'on a sauté, d'un onglet à l'autre.
- Serveurs de langage, facultatifs : Préférences > Serveurs de langage installe et retire celui d'un langage (Rust, Python, TypeScript / JavaScript dont React et Next.js, Go, Shell, PHP, C#) sans droits administrateur, et la barre d'état le propose à l'ouverture d'un fichier. Sans serveur, les définitions sont trouvées par recherche de motifs.
