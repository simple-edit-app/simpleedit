# Journal des modifications

## [1.22.0] - 2026-11-09

### Ajouté
- Console : plusieurs consoles en onglets ; une recherche dans l'historique de ce qui a été exécuté et de quoi remettre une entrée dans l'éditeur ; un bouton Formater pour le SQL ; le mot que le serveur a mis en cause est souligné ; le SQL d'un fichier en cours d'édition s'exécute sur la source de données de la console (Actions : « Exécuter le SQL du fichier »).
- Complétion : après `JOIN`, les tables liées par une clé étrangère viennent en premier ; après `ON`, la condition de la clé étrangère est proposée ; les fonctions des schémas le sont aussi.
- Tables : sélection d'un bloc de cellules (Maj et un clic ou les flèches), copié en TSV, CSV, `INSERT` ou Markdown ; une recherche dans toutes les colonnes, et un champ sous chaque colonne (`> 10`, `= abc`, `~ regex`, `null`), le tout avec le `WHERE` ; une ligne s'ouvre en formulaire (F4), avec une liste pour les booléens, les énumérations et les clés étrangères, et un contrôle et une mise en forme du JSON.
- Import d'un fichier CSV ou JSON dans une table, avec la colonne où va chaque champ, tout ou rien.
- Copie d'une table, avec ou sans ses lignes, vers une autre source de données ; sauvegarde et restauration d'une base, d'un schéma ou d'une table (`pg_dump`, `pg_restore` et `psql` sont cherchés sur la machine, y compris à travers un tunnel SSH).
- Santé d'une base : tailles, lignes mortes, dernier vacuum et analyze, `VACUUM` et `ANALYZE` d'une table, et les index que personne ne lit.
- Une source de données peut être marquée comme production : chaque modification demandée dans la console est confirmée. Sur toute source, un `UPDATE` ou un `DELETE` sans `WHERE` est confirmé d'abord.

### Modifié
- Les lignes liées à une ligne sont listées dans une fenêtre à part (« Lignes liées »), en deux groupes avec leur sens et des conditions courtes, au lieu d'une longue liste dans le menu d'une cellule.
- Le protocole du driver passe à 18 : le driver est téléchargé de nouveau au premier usage.

## [1.21.0] - 2026-11-08

### Ajouté
- Le message « driver non installé » dans l'arbre d'une source de données est cliquable : un clic ouvre les réglages de la source, où le driver se télécharge.

## [1.20.0] - 2026-11-08

### Ajouté
- Les tables et l'arbre sont lus par une session qui reste ouverte : la connexion, le TLS et le tunnel SSH ne sont payés qu'une fois, plus à chaque page.
- Le DDL d'une table couvre les tables partitionnées et leurs partitions, les tables non journalisées et les triggers ; une vue ou une vue matérialisée montre son SQL depuis le menu de l'arbre, et une fonction ou une procédure montre son SQL au clic.
- Arrêter demande au serveur d'annuler la requête : elle s'arrête aussi sur les serveurs antérieurs à PostgreSQL 14.
- La complétion connaît jusqu'à 100 000 colonnes, celles des schémas utilisés d'abord, et dit quand elle est partielle.

### Corrigé
- Un texte à plusieurs commandes (le DDL d'une table avec ses index, par exemple) s'exécute de nouveau dans la console ; il était refusé en 1.19.0.

## [1.19.0] - 2026-11-08

### Ajouté
- La console garde sa session : une transaction commencée dans une exécution, une table temporaire ou un `SET` sont encore là à la suivante. Une transaction ouverte est signalée, avec des boutons Valider et Annuler, et les lignes d'une requête sont lues dans cette même session.
- La console s'exécute dans la base de votre choix (une liste près de la connexion), et ses onglets de résultat y restent.
- Plan et Plan mesuré (requêtes de lecture seulement) dans la console.
- Toutes les lignes d'une table ou d'une requête, avec son filtre et son tri, s'exportent en CSV ou JSON depuis la grille, lues en flux depuis le serveur.

### Modifié
- Les pages d'une table suivent sa clé primaire (la place de la ligne sans clé) : une ligne n'est jamais sur deux pages ni sur aucune.

### Corrigé
- Trier sur un nombre, une date ou toute colonne qui n'est pas du texte la triait comme du texte (10 avant 2).
- Les changements de l'éditeur de table forment une transaction : un échec laisse la table comme elle était.
- Un onglet de résultat qui a écrit (`INSERT … RETURNING`, un plan) n'est plus rejoué par Actualiser.
- Une source en lecture seule refuse maintenant ce qui pourrait écrire ou lever la protection, dans la console et dans le champ `WHERE`.
- Avec un tunnel SSH, l'arbre de plusieurs bases fonctionne avec un port local fixé, et `verify-full` vérifie le certificat contre le nom du serveur.
- Une requête que le serveur ne peut pas préparer dans une transaction ouverte donne sa propre erreur.

## [1.18.0] - 2026-11-08

### Ajouté
- Les bases de données, comme dans un IDE JetBrains : un bouton ouvre un panneau de sources de données. **Seul PostgreSQL est pris en charge dans cette première version.** Son driver est un petit programme séparé, téléchargé et vérifié par signature la première fois.
  - Une source : URL ou hôte, utilisateur et mot de passe (gardé dans le trousseau du système avec *Toujours*), TLS de désactivé à verify-full, tunnel SSH, `.pgpass`, lecture seule, fuseau horaire, script de démarrage, durée maximale des requêtes, filtres de schémas et d'objets, fichier de driver.
  - L'arbre : bases, schémas, tables et vues dépliables sur leurs colonnes, clés et index, fonctions, séquences et types, une zone de recherche, un clic droit pour créer une table ou un schéma.
  - Une table s'ouvre dans un onglet, groupé et coloré par source : pages, tri, `WHERE`, édition cellule par cellule (NULL, sélecteur de date, ajout, suppression, validation en une transaction), tables sans clé primaire aussi, colonnes redimensionnables, visualiseur de valeurs longues et de JSON, copie d'une cellule ou d'une ligne, et lignes liées par clés étrangères, dans les deux sens.
  - Une console : requêtes exécutées (sélection, courante ou toutes), bouton Arrêter, coloration et complétion SQL, journal de ce que l'app exécute.
  - Un éditeur de table : colonnes, types (avec une expression `USING`), valeurs par défaut, clés, contraintes et index modifiés sur place, schéma, nom, commentaires, et le SQL exécuté en une transaction ; le DDL d'une table avec Copier ; vider et supprimer avec confirmation. PostgreSQL ne sait pas déplacer une colonne : les nouvelles sont ajoutées à la fin.
- Une source de données demande un second clic avant d'être supprimée.

## [1.17.0] - 2026-11-07

### Ajouté
- Sur macOS, les menus (Fichier, Édition, Affichage, Aide) sont dans la barre de menus du système, à côté du menu de l'application avec À propos, Préférences et Quitter.
- Dans le panneau Git, les actions de l'onglet Branches se redimensionnent en tirant leur bord gauche ; la largeur est retenue.
- Dans le menu Affichage, Sombre et Clair sont deux entrées, la courante étant cochée.

### Modifié
- Dans le panneau Git, Remotes et Dépôts sont dans l'onglet Branches, Patchs dans l'onglet Commit ; Fetch n'est plus répété dans l'en-tête.
- Les popins des outils Git se ferment avec une croix, et leurs boutons secondaires sont cerclés comme le reste de l'app.
- Sur macOS, la fenêtre n'a plus de barre de titre à elle.

## [1.16.1] - 2026-11-06

### Corrigé
- Dans la recherche partout, le bouton qui envoie les résultats au panneau Résultats le dit.

## [1.16.0] - 2026-11-06

### Ajouté
- Historique local, comme dans IntelliJ IDEA : ce que les fichiers ont été, gardé sur cette machine quoi que sache Git, pendant 5 jours travaillés par défaut (Préférences > Git, Désactivé pour l'arrêter). Voir l'historique depuis l'éditeur, l'arbre ou la liste des fichiers, pour un fichier ou un dossier : révisions avec recherche dans leur contenu, les deux fichiers côte à côte avec des flèches pour rétablir un changement, ou vue compacte ; restauration d'une version, création de patch, fichiers supprimés récupérés. Labels à la main ou automatiques à chaque commit.
- Git, d'après IntelliJ IDEA :
  - Renommer une branche ; comparer une branche à la branche courante, dans le log, dans les deux sens.
  - Récupérer un fichier d'une branche ou d'une révision ; historique d'une sélection.
  - Options de commit : Signed-off-by, ignorer les hooks, autre auteur, messages récents.
  - Boîte de push : les commits qui partent, le remote et la branche, les tags ; pousser jusqu'à un commit. Un push refusé propose de rebaser ou fusionner, puis repousse.
  - Remotes : ajouter, modifier, supprimer.
  - Filtres du log, tous à la fois : auteur, dates, dossier ou fichier, premier parent, sans fusions.
  - Fixup et Squash Into depuis un commit ; retirer des fichiers d'un commit ou les extraire dans leur propre commit ; appliquer les changements de quelques fichiers d'un commit sans commiter.
  - Blame : ignorer les espaces, détecter les déplacements, masquer un commit.
  - Patchs : copier ou enregistrer les modifications locales et les commits, appliquer depuis un fichier ou le presse-papiers.
  - Fusion et rebase avec options (`--no-commit`, `--onto`, `--update-refs` et d'autres).
  - Dépôts : l'état de chaque projet, le choix de celui affiché, fetch, pull ou push sur tous.

### Modifié
- Ctrl+Maj+K ouvre la boîte de push.
- La recherche partout et Chercher dans les fichiers ont un bouton de fermeture.

### Corrigé
- Dans un diff, les flèches ramènent à l'écran un changement isolé.
- Les longues lignes ne passent plus sous les boutons du log et des remotes.

## [1.15.1] - 2026-11-06

### Ajouté
- Clic droit sur l'étiquette d'un projet dans les onglets : Fermer tous les onglets de ce projet. Une confirmation est demandée pour les fichiers non enregistrés, et l'étiquette des autres fichiers ferme ses onglets de la même façon.

## [1.15.0] - 2026-11-06

### Ajouté
- Onglets par projet : chaque projet a une couleur, en liseré sur ses onglets et en pastille dans l'arbre des projets et la liste des onglets. Avec des fichiers de plusieurs projets ouverts, les onglets d'un projet sont côte à côte derrière une étiquette à sa couleur. Un clic sur l'étiquette replie ou ouvre le groupe, la glisser déplace tout le groupe, et un clic droit change la couleur du projet. Un onglet lâché parmi ceux d'un autre projet revient dans son groupe.

### Modifié
- Dans un groupe, les onglets n'affichent le dossier que si deux de ses fichiers portent le même nom.

## [1.14.0] - 2026-11-06

### Ajouté
- Remplacer dans les fichiers, comme dans IntelliJ IDEA : Ctrl+Maj+R, ou le nouveau Remplacer dans ce dossier de l'arbre du projet. Avec une expression régulière, `$1` et `${nom}` donnent les groupes, et `\l \u \L \U \E` changent la casse ; Conserver la casse garde la casse de ce qui est remplacé. Chaque correspondance montre son remplacement avant toute écriture. Remplacer traite une correspondance, Tout remplacer les autres après confirmation ; les fichiers ouverts dans l'éditeur sont modifiés là (Ctrl+Z annule), les autres sur le disque (Annuler le remplacement).
- Panneau Résultats, comme la fenêtre Find d'IntelliJ : Ctrl+Entrée y envoie les correspondances, en onglets épinglables, par fichier ou par dossier. Suppr exclut une correspondance, Inser la réinclut, Ctrl+X la retire, Alt+L et Alt+D remplacent, Ctrl+F5 relance la recherche, Ctrl+Alt+↑/↓ parcourent les correspondances, Alt+O les exporte.

### Modifié
- Chercher dans les fichiers a été refait : mots entiers, recherche sur plusieurs lignes, masque de fichiers (`*.js, !test*.js`), dans ou hors des commentaires et des chaînes, aperçu des lignes autour de chaque correspondance, épingle, recherches récentes (Alt+↓). La recherche porte sur le projet, un dossier avec ou sans ses sous-dossiers, ou les fichiers ouverts, récents, modifiés ou courant.
- Chercher dans les fichiers et la recherche partout trouvent maintenant les fichiers que Git ignore un par un, comme un `.env`, mais pas les dossiers qu'il ignore entiers. Dans Chercher dans les fichiers, les fichiers ignorés par Git peuvent être désactivés.
- La recherche partout envoie ses résultats au panneau Résultats.

## [1.13.0] - 2026-11-05

### Ajouté
- Recherche partout, comme dans IntelliJ IDEA : Maj deux fois ouvre un champ qui trouve classes, fichiers et dossiers, symboles, actions, réglages et texte, par onglet (Tout, Classes, Fichiers, Symboles, Actions, Texte, et Git une fois activé dans les Préférences) ou tout à la fois. Sans rien taper, elle liste les fichiers récents. Maj deux fois encore (ou Alt+N) inclut les éléments hors projet : ce que Git ignore, les bibliothèques. `nom:ligne:colonne` ouvre un fichier à un endroit, `Type.nom` trouve un nom qualifié, `/` liste les réglages (basculés ON/OFF sur place) et les pages des Préférences, un calcul (`2^10`, `sqrt(2)`, `0xff + 1`) est fait et copié avec Entrée. Les actions se trouvent par leurs mots dans n'importe quel ordre, par synonymes et par des abréviations à soi (clic droit sur une action), et Alt+Entrée définit le raccourci d'une action. Les résultats se filtrent par type de fichier et s'ouvrent dans Chercher dans les fichiers. Tab et Maj+Tab changent d'onglet, Ctrl+↑/↓ vont au premier et au dernier résultat.
- Raccourcis des onglets : Ctrl+Maj+N (fichiers), Ctrl+Alt+N (classes), Ctrl+Alt+Maj+N (symboles), Ctrl+Maj+A (actions).

### Modifié
- Aller à un fichier et la palette de commandes sont maintenant les onglets Fichiers et Actions de la recherche partout.

## [1.12.0] - 2026-11-05

### Ajouté
- Complétion du code par les serveurs de langage (TypeScript/JavaScript, Rust, Python, Go, C/C++, Java et les autres que l'éditeur connaît). La liste s'ouvre en tapant un nom ou après `.`, `::`… (Ctrl+Espace pour la demander), ne contient que ce que le serveur propose, filtré par ce qui est tapé, dans l'ordre du serveur, avec le type de chaque élément, sa signature et sa documentation. Entrée insère l'élément, Tab remplace la fin du mot ; les imports sont ajoutés avec lui (une seule annulation), et les arguments d'une fonction se remplissent avec Tab. Une liste que le serveur annonce incomplète est redemandée pendant la frappe, et tant que le serveur charge le projet la liste l'indique puis est redemandée quand il a fini. Sans serveur de langage, rien n'est proposé.
- Infos sur les paramètres : la signature de l'appel en cours, avec le paramètre sous le curseur mis en avant, s'ouvre sur `(` et `,` (Ctrl+P pour la demander) ; Alt+↑/↓ parcourt les surcharges.
- Préférences > Éditeur : complétion activée ou non, pendant la frappe ou seulement au raccourci, et par langage : imports depuis les autres modules (TypeScript/JavaScript, Rust, Python), complétions postfixes et arguments des appels (Rust).
- Les serveurs de langage démarrent à l'ouverture d'un fichier de leur projet et le chargent en arrière-plan ; à chaque frappe, seule la partie modifiée leur est envoyée.

## [1.11.0] - 2026-11-05

### Modifié
- Un nouveau look : le fond est le gris foncé d'IntelliJ, les cadres (fichiers, éditeur, barre des panneaux) se touchent et sont séparés par des traits fins, comme dans Zed, et plus rien n'est arrondi (boutons, champs, menus, cartes, surlignages).
- Les onglets suivent Zed : une barre un peu plus claire que l'éditeur, des traits fins entre les onglets, et l'onglet ouvert de la couleur de l'éditeur, dans son prolongement.

### Retiré
- Le bouton « Formater le code » au-dessus de l'éditeur : il est dans le menu Édition, dans le menu du clic droit de l'éditeur, dans la palette et a son raccourci.

## [1.10.0] - 2026-11-05

### Ajouté
- Des icônes colorées devant le nom des fichiers, selon leur type (Rust, TypeScript, JSON, Docker, npm, YAML, Markdown, images…), dans l'arborescence, les fichiers ouverts, les onglets, les panneaux Git, scratch et signets, Aller à un fichier et Chercher dans les fichiers ; les dossiers ont un contour fin. Les icônes viennent du jeu Seti de la Nerd Font et de Lucide.

### Corrigé
- Un projet ouvert par un lien symbolique (et tout projet sous `/var` sur macOS) est de nouveau coloré selon Git dans l'arborescence.

## [1.9.0] - 2026-11-05

### Ajouté
- Projets : les dossiers ajoutés à la sidebar (Fichier > Ajouter un dossier, ou le `+` du panneau Projets) s'affichent en arborescence, lue au fil de l'ouverture des dossiers et tenue à jour quand le disque change. Depuis l'arbre : nouveau fichier, nouveau dossier, renommer, dupliquer, supprimer (un dossier après confirmation), déplacer en glissant sur un dossier, copier le chemin, ouvrir un terminal ici, afficher dans le gestionnaire de fichiers, chercher dans ce dossier. Les flèches se déplacent dans l'arbre et Entrée ouvre. Tout replier, masquer ce que Git ignore, et des noms à exclure (`node_modules/, *.log`) dans Préférences > Éditeur.
- Une barre à gauche ouvre le panneau Fichiers ou Projets (un à la fois) et regroupe les boutons du terminal, des fichiers scratch, des signets et de Git ; la barre sous la liste des fichiers disparaît.
- Des onglets au-dessus de l'éditeur : ils défilent à la molette, se réordonnent en glissant, se ferment au clic du milieu, montrent le type de fichier, et une liste (`⌄`) les regroupe tous. Ctrl+Tab va à l'onglet vu avant (plus loin en gardant Ctrl), Ctrl+Maj+T rouvre l'onglet fermé. Un fichier ouvert depuis l'arbre prend un onglet d'aperçu (en italique), gardé dès qu'il est modifié ou ouvert d'un double-clic.
- Aller à un fichier (Ctrl+Maj+N) : un fichier des projets retrouvé par son nom. Chercher dans les fichiers (Ctrl+Maj+F) : un texte dans tous les fichiers des projets, ou du dossier choisi dans l'arbre, avec la casse et les expressions régulières.
- Les fichiers sont colorés selon leur état Git dans l'arbre et sur les onglets (modifié, nouveau, en conflit, ignoré), et un point rouge marque les fichiers où le lint a trouvé des erreurs.

### Corrigé
- Un menu ouvert ne se ferme plus tout seul pendant qu'on le parcourt à la souris.

## [1.8.1] - 2026-11-04

### Corrigé
- L'espace entre la liste des fichiers et l'éditeur était deux fois plus large que les autres ; il est maintenant d'un seul espace, et tout cet espace permet de redimensionner la liste.

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
