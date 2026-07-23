app-title = dbboard

tables-heading = Tables
tables-empty = (aucune table)
tables-context-select = Sélectionner toutes les lignes
tables-context-count = Compter les lignes

sql-heading = SQL
sql-run-button = Exécuter

history-title = Historique ({ $count })
history-empty = (aucune requête récente)

result-heading = Résultat
result-empty = (exécutez une requête)
result-affected = OK ({ $rows } lignes affectées)
result-copy-all = Copier
result-copy-all-hint = Copier tout le résultat dans le presse-papiers au format TSV (collez-le dans un tableur)
result-export-csv = Enregistrer CSV…
result-export-error = Impossible d'enregistrer le fichier CSV
result-copy-selected = Copier la sélection
result-copy-selected-hint = Copier les lignes sélectionnées dans le presse-papiers au format TSV
result-export-selected-csv = Enregistrer la sélection en CSV…
result-clear-selection = Effacer la sélection
result-selected-count = { $count } sélectionnée(s)
result-select-row-hint = Cliquer pour sélectionner la ligne (Ctrl / Maj pour plusieurs)
result-sort-hint = Cliquez pour trier ; Ctrl / Maj pour ajouter un niveau

error-prefix-connection = Erreur de connexion
error-prefix-query = Erreur de requête
error-prefix-schema = Erreur de schéma
error-prefix-type-conversion = Erreur de conversion de type
error-prefix-capability = Fonctionnalité indisponible

connections-window-title = Connexions
connections-restart-hint = Les modifications prennent effet au prochain lancement de dbboard.
connections-list-empty = (aucune connexion configurée)
connections-add-button = Ajouter
connections-edit-button = Modifier
connections-delete-button = Supprimer
connections-save-button = Enregistrer
connections-cancel-button = Annuler
connections-confirm-delete = Supprimer cette connexion ?
connections-field-id = Identifiant
connections-field-name = Nom
connections-field-kind = Type
connections-field-turso-path = Chemin de la base
connections-field-d1-account = Identifiant du compte
connections-field-d1-database = Identifiant de la base
connections-field-d1-base-url = URL de base (optionnel)
connections-field-d1-token = Jeton API
connections-field-pg-url = URL de connexion
connections-replace-token = Remplacer le jeton
connections-replace-url = Remplacer l'URL
connections-connect-button = Connecter
connections-reconnect-button = Reconnecter
connections-active-marker = (active)
connections-switch-error = Échec de la connexion

language-menu = Langue
theme-menu = Thème
theme-auto = Automatique
theme-light = Clair
theme-dark = Sombre
help-menu = Aide
help-docs-hint = Consultez README.md et docs/ pour la configuration et les guides de connexion.
help-repo-link = Projet sur GitHub
help-ai-about-title = À propos de l'Assistant IA
help-ai-about-body = L'Assistant IA explique une instruction SQL en langage clair et rédige une requête SQL à partir d'une description que vous saisissez ; pour les suggestions, il lit aussi les noms de vos tables et colonnes. Il n'exécute jamais de SQL, n'écrit jamais dans votre base de données et n'envoie jamais de lignes de données où que ce soit : rien ne se produit tant que vous n'avez pas copié un brouillon dans l'éditeur pour l'exécuter vous-même. Une clé API est requise et est stockée dans le gestionnaire d'identifiants de votre système d'exploitation.

ai-menu-button = Assistant IA
ai-panel-title = Assistant IA
ai-scope-hint = Explique le SQL et rédige des requêtes à partir d'une description. Il n'exécute jamais de SQL et ne modifie aucune donnée : vous vérifiez et exécutez tout vous-même.
ai-mode-explain = Expliquer le SQL
ai-mode-suggest = Suggérer du SQL
ai-input-explain = SQL à expliquer :
ai-input-suggest = Décrivez la requête souhaitée :
ai-send-button = Envoyer
ai-busy = En attente du fournisseur…
ai-empty = (Aucune réponse — saisissez une invite ci-dessus et appuyez sur Envoyer)
ai-error-prefix-configuration = Erreur de configuration IA
ai-error-prefix-network = Erreur réseau IA
ai-error-prefix-provider = Erreur du fournisseur IA
ai-error-prefix-quota = Quota IA dépassé
ai-error-prefix-cancelled = Requête IA annulée

# ADR-0026 Phase 4 Stage 2 Group B : streaming + annulation coopérative
# + compteur de jetons.
ai-cancel-button = Annuler
ai-cancelled-message = Annulé.
ai-tokens-meter = Jetons : { $tin } entrée / { $tout } sortie

# ADR-0025 Phase 4 Stage 2 Group A slice (b) : fenêtre de paramètres des fournisseurs IA.
ai-settings-menu-button = Fournisseurs IA
ai-settings-window-title = Fournisseurs IA
ai-settings-list-empty = (aucun fournisseur IA configuré)
ai-settings-add-button = Ajouter
ai-settings-edit-button = Modifier
ai-settings-delete-button = Supprimer
ai-settings-save-button = Enregistrer
ai-settings-cancel-button = Annuler
ai-settings-use-button = Utiliser
ai-settings-confirm-delete = Supprimer ce fournisseur IA ?
ai-settings-active-marker = (actif)
ai-settings-field-id = Id
ai-settings-field-name = Nom
ai-settings-field-kind = Type
ai-settings-field-model = Modèle (optionnel)
ai-settings-field-api-key = Clé API
ai-settings-replace-api-key = Remplacer la clé API
ai-settings-kind-anthropic = Anthropic
ai-active-with-name = Actif : { $name }

ai-include-details = Inclure les détails des colonnes
ai-prefetching = Récupération des schémas de tables…
ai-prefetch-warning = Impossible de décrire { $count } table(s) ; la suggestion continue sans elles.

# ADR-0030 result grid: truncated long / multi-line cell values.
cell-expand-hint = Afficher la valeur complète
cell-full-text-title = Valeur de la cellule
cell-copy = Copier

# ADR-0030 auto-limit guard for bare SELECTs.
auto-limit-checkbox = LIMIT { $count }
auto-limit-hint = Ajoute un LIMIT aux SELECT sans limite pour éviter quun balayage illimité ne fige linterface. Écrivez votre propre LIMIT ou décochez pour passer outre.

# ADR-0031 structure tab.
tab-results = Résultat
tab-structure = Structure
structure-empty = (cliquez sur une table pour voir sa structure)
structure-loading = Description de la table…
structure-no-columns = (aucune colonne)
structure-col-ordinal = #
structure-col-name = Nom
structure-col-type = Type
structure-col-nullable = Null
structure-col-pk = Clé
structure-col-default = Défaut
structure-col-note = Note
structure-note-hint = Ajouter une note…
structure-table-note = Note de table

edit-save-button = Enregistrer
edit-discard-button = Annuler
edit-staged-count = { $count } modification(s) en attente
edit-set-null = Définir NULL
edit-revert-cell = Rétablir la cellule
edit-cell-hint = Double-cliquez pour modifier · clic droit pour NULL
edit-save-unexpected-rows = Enregistrement arrêté : 1 ligne attendue, { $rows } affectée(s)

# ADR-0049 backup (logical dump).
backup-button = Sauvegarde…
backup-button-hint = Exporter les tables de cette base de données dans un fichier SQL
backup-planning = Préparation de la sauvegarde…
backup-warn-title = Base de données volumineuse
backup-warn-body = Cette base de données compte { $rows } lignes sur l'ensemble de ses tables. L'export peut être long et produire un fichier volumineux.
backup-warn-continue = Sauvegarder quand même
backup-warn-cancel = Annuler
backup-dialog-title = Enregistrer la sauvegarde sous
backup-progress-title = Sauvegarde en cours
backup-progress-table = Table { $done } sur { $total }
backup-progress-rows = { $done } / { $total } lignes
backup-progress-current = En cours : { $table }
backup-cancel-button = Annuler
backup-done-title = Sauvegarde terminée
backup-done-summary = { $tables } table(s), { $rows } lignes exportées.
backup-done-cancelled = Sauvegarde annulée — le fichier contient un export partiel.
backup-done-failures = { $count } table(s) n'ont pas pu être lues et ont été ignorées.
backup-done-truncations = { $count } table(s) ont été tronquées en cours de route.
backup-failed-title = Échec de la sauvegarde
backup-close-button = Fermer
