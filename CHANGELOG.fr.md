# Journal des modifications

## [1.12.0] - 2026-10-20

### Ajouté
- Pliage de code : une flèche dans la marge plie ou déplie les blocs entre accolades (fonctions, objets et tableaux JSON), les commentaires, les blocs indentés (Python, YAML, HTML…), les sections Markdown et les blocs de code. Ctrl+clic plie aussi les blocs du même niveau, Alt+clic tout ce qu'ils contiennent ; une infobulle sur la flèche rappelle les touches.
- « Replier / déplier le bloc » (Ctrl+Maj+L, modifiable), « Tout replier » et « Tout déplier » dans le menu Édition et la palette de commandes. Les flèches du clavier sautent les lignes pliées ; écrire dans un bloc plié, une recherche, « Aller à la ligne » ou un marque-page l'ouvre.

## [1.11.0] - 2026-10-20

### Ajouté
- Le menu Fichier range les fichiers récents dans un sous-menu « Fichiers récents » qui s'ouvre au survol, avec une entrée « Effacer » pour vider la liste.

## [1.10.0] - 2026-10-20

### Corrigé
- Les fichiers Windows (CRLF) ne sont plus doublés à l'enregistrement, et l'absence de saut de ligne final est conservée.
- Les tabulations ne sont plus perdues : Tab tape une vraie tabulation en mode tabulations, et les tabulations finales survivent au collage, à l'annulation et au formatage.
- Un résultat de formatage ne peut plus atterrir dans un autre onglet ni écraser ce que vous tapez entre-temps.
- Un fichier modifié sur le disque pendant qu'il était dans un onglet en arrière-plan est maintenant détecté avant que vous l'écrasiez.
- Les chemins relatifs donnés en ligne de commande sont résolus, donc la session ne peut plus enregistrer dans le mauvais fichier ; plusieurs fichiers peuvent s'ouvrir d'un coup.
- Un fichier de session abîmé est mis de côté au lieu d'être écrasé, et quitter prévient si la session n'a pas pu être enregistrée.
- Fermer le dernier onglet laisse un onglet pour écrire ; « Enregistrer sous » vers un onglet ouvert avec des modifications est refusé.
- Alt+Haut / Alt+Bas déplacent la ligne, et les raccourcis fonctionnent même quand le champ de recherche a le focus.
- La recherche sélectionne le bon texte après un emoji, un accent ou du texte de droite à gauche ; « Tout remplacer » ne supprime plus une dernière ligne vide.
- Annuler, rétablir et les commandes de ligne gardent le curseur, la sélection et les marque-pages ; revenir au texte enregistré retire la marque « modifié ».
- Commenter ne casse plus les commentaires de documentation ni les shebangs ; Kotlin, TSX et JSX se formatent correctement.
- Les noms en chinois, japonais, arabe ou avec emoji s'affichent correctement dans l'interface.

### Ajouté
- Coloration syntaxique pour TypeScript, TSX, JSX, TOML, Swift et Kotlin, et davantage de types de fichiers reconnus par leur nom ou leur shebang.
- Préférence pour désactiver le retour à la ligne (plus rapide sur les très longues lignes), et une suggestion de formater un fichier qui en contient une.
- La palette de commandes ignore les accents : « pref » trouve « Préférences ».
- Chaque onglet retient la position du curseur et du défilement.

### Modifié
- Beaucoup plus rapide sur les gros fichiers : aller à une ligne, annuler, commandes de ligne, recherche, enregistrement automatique et historique d'annulation.
- L'installateur en ligne de commande et la mise à jour vérifient strictement les signatures ; Prettier n'est plus téléchargé dans votre dos ; les formateurs s'exécutent dans le dossier du fichier, donc les réglages du projet s'appliquent.
- Basé sur iced 0.14, avec des dépendances à jour qui corrigent des failles de sécurité connues.

## [1.9.0] - 2026-10-19

### Ajouté
- Journal des modifications, en anglais et en français : à ouvrir depuis Aide, « Nouveautés… ».
- La fenêtre de mise à jour liste ce que change la nouvelle version, et SimpleEdit affiche les nouveautés au premier lancement après une mise à jour.

## [1.8.0] - 2026-10-19

### Ajouté
- Marque-pages : repérez une ligne pour la retrouver. Basculez avec Ctrl+F2, naviguez avec F2 / Maj+F2, ou passez par le menu Édition et le menu du clic droit (ajouter, supprimer, tout supprimer).
- Les marque-pages apparaissent sous forme de drapeau dans la gouttière et de trait bleu dans la minimap.
- Les marque-pages sont enregistrés par fichier et réapparaissent à la réouverture du fichier ou de l'app ; ils suivent le texte quand des lignes sont ajoutées ou supprimées.
- Le clic droit place désormais d'abord le curseur sur la ligne cliquée.

## [1.7.0] - 2026-10-19

### Ajouté
- Minimap sur le bord droit : cliquez ou glissez pour vous déplacer dans le fichier ; la zone visible est cadrée en orange et les résultats de recherche sont marqués.
- Option pour masquer la minimap dans les Préférences.

### Corrigé
- Les longs chemins de dossier ne repoussent plus le bouton Formater hors de la barre d'outils.
- Un clic lointain dans un gros fichier ne fige plus les builds de développement.

## [1.6.0] - 2026-10-19

### Modifié
- Nouveau design « Nocturne » : thème, polices, panneaux flottants, palette de commandes.

### Corrigé
- La gouttière des numéros de ligne reste alignée avec les lignes repliées et le défilement.
- Corrections issues d'un audit : perte de données, recherche, sécurité des mises à jour, survol des menus.

## [1.5.0] - 2026-09-03

### Ajouté
- Boutons de sensibilité à la casse et d'expression régulière dans la barre de recherche ; résultats plus visibles.

## [1.4.2] - 2026-09-03

### Corrigé
- Le bouton Remplacer faisait planter l'app.

## [1.4.1] - 2026-09-03

### Ajouté
- Bouton « remplacer un » dans la barre de recherche.

### Corrigé
- La coloration syntaxique était invisible dans certains cas.

## [1.4.0] - 2026-09-03

### Ajouté
- Vraie recherche avec suivant et précédent dans la barre de recherche.

## [1.3.4] - 2026-09-03

### Corrigé
- Survoler un tableau Markdown bloquait le défilement de la page.

## [1.3.3] - 2026-09-03

### Modifié
- Nouvel aspect des tableaux dans l'aperçu Markdown ; l'aperçu n'a plus de menu clic droit.

## [1.3.2] - 2026-09-03

### Corrigé
- Les tableaux Markdown s'affichaient vides ou écrasés.

## [1.3.1] - 2026-09-03

### Corrigé
- L'app ne redémarrait pas après une mise à jour.
- Icône manquante dans l'arbre JSON.

## [1.3.0] - 2026-09-03

### Ajouté
- Les tableaux s'affichent dans l'aperçu Markdown.

## [1.2.0] - 2026-09-03

### Ajouté
- Liens d'ancre cliquables dans l'aperçu Markdown.

## [1.1.3] - 2026-09-03

### Corrigé
- Les menus déroulants étaient décalés quelle que soit la langue.

## [1.1.2] - 2026-09-03

### Corrigé
- Les menus déroulants du haut s'ouvraient décalés vers la droite.

## [1.1.1] - 2026-09-03

### Corrigé
- Le menu du haut se refermait aussitôt après son ouverture.

## [1.1.0] - 2026-09-03

### Ajouté
- Volets d'aperçu Markdown et JSON.

## [1.0.1] - 2026-08-31

### Corrigé
- Le désinstalleur gère AppImage, snap et les binaires isolés.
- L'installeur macOS ne déclenche plus l'avertissement Homebrew sur les taps.

## [1.0.0] - 2026-08-20

### Ajouté
- Première version : éditeur de texte à onglets avec coloration syntaxique, recherche, formatage, sessions, thèmes et interface en anglais et en français.
