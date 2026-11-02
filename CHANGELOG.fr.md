# Journal des modifications

## [1.0.0] - 2026-11-02

### Ajouté
- Première version de SimpleEdit : un éditeur de texte rapide, stable et multiplateforme pour macOS (Apple Silicon) et Linux (x86_64), écrit en Rust.
- Édition : coloration syntaxique pour plus de 30 langages, recherche et remplacement avec expressions régulières, opérations sur les lignes, pliage de code, marque-pages, aperçu Markdown, formatage du code par des outils externes, thèmes sombre et clair, français et anglais, et une session qui conserve le texte non enregistré entre deux lancements.
- Aller à la définition : Ctrl+clic (Cmd+clic sur macOS) sur une fonction, une variable, une classe, une interface… mène à sa définition, et sur une définition il liste les endroits où elle est utilisée. Il ne répond que s'il est sûr de lui : jamais de résultat faux.
- Retour / Avance (Alt+Gauche / Alt+Droite, boutons de la souris, menu et palette de commandes) parcourent les endroits d'où l'on a sauté, d'un onglet à l'autre.
- Serveurs de langage, facultatifs : Préférences > Serveurs de langage installe et retire celui d'un langage (Rust, Python, TypeScript / JavaScript dont React et Next.js, Go, Shell, PHP, C#) sans droits administrateur, et la barre d'état le propose à l'ouverture d'un fichier. Sans serveur, les définitions sont trouvées par recherche de motifs.
