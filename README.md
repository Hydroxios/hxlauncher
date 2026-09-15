# HX Launcher

Launcher **Minecraft Java Edition** avec **Tauri 2**, **React / TypeScript** et un backend **Rust**. Interface en français, instances isolées et installation par import CurseForge.

## Démarrer

Prérequis : Node.js 22.12+ (ou 24+), Rust stable récent, [prérequis Tauri](https://v2.tauri.app/start/prerequisites/) et Java pour lancer Minecraft.

```sh
npm install
npm run tauri dev
```

Le serveur Vite utilise `127.0.0.1:15420`. `npm run dev` affiche seulement l’interface dans un navigateur : les commandes natives nécessitent Tauri.

```sh
npm run build                          # TypeScript + frontend
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build                    # Bundle de production pour la machine actuelle
npm run tauri build -- --debug --bundles app  # .app macOS de développement
```

## Ce qui est implémenté

- Connexion Microsoft par code dans le navigateur système.
- **`oauth2` 5** gère le device flow, l’attente avec backoff, l’expiration et le renouvellement Microsoft.
- **`minecraft-msa-auth` 0.5** gère les échanges Microsoft → Xbox Live → Minecraft Services.
- Lecture du profil Minecraft Java ; le frontend ne reçoit aucun access token ni refresh token.
- Refresh token dans le trousseau système via `keyring` ; renouvellement au démarrage et avant chaque lancement, déconnexion et annulation de connexion.
- Liste des versions stables Mojang à partir de Minecraft 1.19 ; création d’instances et réglage Java/RAM.
- Téléchargement du client, des bibliothèques et des assets depuis les manifestes Mojang, contrôle SHA-1, cache partagé et 12 téléchargements d’assets simultanés.
- Construction des arguments JVM/jeu, règles de plateforme, lancement Java et suivi du processus. Les anciens formats de natives incompatibles avec ARM sont refusés explicitement.
- Lecture d’un **ZIP CurseForge local** : manifeste v1, version Minecraft, modloader primaire, références `projectID` / `fileID`, nombre d’overrides. Rejet des chemins sortants, des liens symboliques et des archives démesurées.

## Configurer Microsoft

La bibliothèque d’authentification n’élimine pas la nécessité d’identifier ton application auprès de Microsoft.

1. Dans **Microsoft Entra → Inscriptions d’applications**, crée une application acceptant les **comptes Microsoft personnels**.
2. Dans **Authentification → Paramètres avancés**, active **Autoriser les flux clients publics**. Aucun secret client ni URL de redirection n’est nécessaire pour le device flow.
3. Renseigne `MICROSOFT_CLIENT_ID` dans `.env` à la racine, puis recompile le launcher.
4. Clique sur **Se connecter avec Microsoft**, ouvre Microsoft et saisis le code.
5. Le compte doit avoir accès à Minecraft Java et avoir créé son profil de joueur.

L’accès à Minecraft Services pour une nouvelle application peut nécessiter l’autorisation de Mojang. Un refus à cette étape n’est pas résolu en changeant de bibliothèque ou en empruntant le Client ID d’un autre launcher. La connexion complète doit être validée avec ton application et ton compte avant distribution.

En cas de refus, HX distingue désormais Xbox Live, Xbox XSTS et Minecraft Services. Un HTTP 403 seul ne prouve pas un problème d’inscription. Si Minecraft renvoie `Invalid app registration`, utilise le [formulaire officiel AppID Review](https://aka.ms/mce-reviewappid) : il demande le Client ID, le Tenant ID, un contact, une page présentant l’application et une justification de l’accès aux API. Le formulaire indique un examen hebdomadaire, sans garantie de délai d’approbation. Les réponses et jetons d’authentification ne sont pas exposés dans le diagnostic.

Sources : [device flow Microsoft](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code), [oauth2-rs](https://github.com/ramosbugs/oauth2-rs), [minecraft-msa-auth](https://github.com/minecraft-rs/minecraft-msa-auth).

## Java

Java n’est pas encore installé automatiquement. Renseigne `java` ou le chemin absolu de ton exécutable dans les paramètres, puis **Vérifier**. Le backend compare la version de Java au minimum demandé par le manifeste Minecraft avant tout téléchargement du jeu. Utilise un Java adapté à l’architecture de ton ordinateur.

Exemples : Java 17 pour Minecraft 1.20.1 ; Java 21 pour 1.20.5 / 1.21. Les versions ultérieures suivent leur manifeste.

## Données

Dossier géré par Tauri, affiché dans les paramètres. Sur macOS :

```text
~/Library/Application Support/dev.hydro.hxlauncher/
├── state.json                  # Paramètres non secrets et instances
├── minecraft/
│   ├── versions/               # Manifeste et client Minecraft
│   ├── libraries/
│   └── assets/
└── instances/<id>/             # Dossier de jeu isolé
    ├── natives/
    ├── saves/                  # Créé par Minecraft
    └── launcher-game.log       # stdout/stderr du jeu
```

Les jetons persistants restent dans le trousseau système, jamais dans `state.json`. Le launcher ne journalise pas la ligne de commande Java, qui contient le jeton de session. Les logs de jeu sont locaux.

## Configuration du launcher

Les utilisateurs n’ont aucun identifiant technique à saisir dans les réglages. Renseigne le `.env` à la racine (voir `.env.example`) avant `npm run tauri dev` ou un build. Après chaque modification, relance `npm run tauri dev` : le build Rust incorpore les valeurs au démarrage de l’application.

```dotenv
MICROSOFT_CLIENT_ID=ton-id-application
CURSEFORGE_API_KEY=ta-cle-api
```

Les variables du processus sont prioritaires. Le build Rust charge `.env` et incorpore ces deux valeurs dans le binaire ; elles ne transitent pas par React. Modifier `.env` nécessite une recompilation. Le fichier est ignoré par Git. Une clé incorporée dans une application distribuée reste extractible : pour garder une clé réellement secrète en production, les appels CurseForge doivent passer par un service backend.

## Import CurseForge

**Modpacks → Choisir un ZIP → Installer.** L’import suit les versions exactes du manifeste, télécharge les fichiers avec l’API officielle et contrôle leur SHA-1. Les mods vont dans `mods`, les packs de ressources dans `resourcepacks`, les shaders dans `shaderpacks`. Les configurations sont extraites dans un dossier temporaire ; l’instance apparaît à l’accueil uniquement après installation complète.

Fabric, Quilt, Forge et NeoForge sont installés via `mc-launcher-core` 0.1.2 et les sources officielles. Les installateurs Forge/NeoForge utilisent le Java configuré, avec journal `modded-runtime/loader-install.log` et délai maximal de 15 minutes. Le moteur partagé et les dossiers de jeu sont séparés. Une licence Minecraft et une connexion Microsoft sont nécessaires pour jouer, pas pour installer.

L’absence de clé, un téléchargement interdit par l’auteur, une empreinte manquante ou un fichier en double bloque l’import avec une erreur explicite. Aucun mod référencé n’est ignoré silencieusement, y compris les fichiers facultatifs. En cas d’échec, les fichiers temporaires sont nettoyés et aucune instance incomplète n’est publiée. Les ressources Minecraft déjà téléchargées restent en cache pour une nouvelle tentative. Reprise des mods et annulation ne sont pas encore proposées.

## Structure

- `src/App.tsx` : bibliothèque, compte, import, paramètres, activité.
- `src/Player.tsx` : personnage 3D animé avec `skinview3d`, modèles classic/slim et gestion de la durée de vie WebGL.
- `public/skins/steve.png` : texture Steve extraite du client officiel Mojang 1.21.1 (asset Minecraft, propriété de Mojang/Microsoft).
- `src-tauri/src/auth.rs` : intégration des bibliothèques d’authentification et du trousseau.
- `src-tauri/src/minecraft.rs` : manifests, téléchargement, règles et lancement.
- `src-tauri/src/packs.rs` : contrôle des archives, API CurseForge et publication des instances.
- `src-tauri/src/modded.rs` : installation des modloaders et lancement des packs.
- `src-tauri/src/lib.rs` : commandes Tauri et persistance.

## Validation et limites

La compilation frontend et 11 tests Rust couvrent les chemins sûrs, règles/arguments de lancement, manifestes CurseForge, routage des fichiers et extraction sans écrasement. Un test réseau séparé vérifie les métadonnées officielles Mojang/Fabric et la construction de la commande de lancement. La connexion réelle, le téléchargement intégral et une partie Minecraft nécessitent ton compte ; ils ne sont pas validés par ces tests. Les builds Windows/Linux nécessitent leurs propres essais. Le bundle de développement macOS n’est pas signé pour distribution.

Pas encore : installation automatique de Java, gestion des snapshots et anciennes versions, suppression/duplication des instances, import depuis URL, réparation et annulation d’installation.

## Interface pastel et skin

Fenêtre sans décorations natives, barre de déplacement et boutons réduire/agrandir/fermer personnalisés. Fond rose, bleu et sable avec panneaux translucides. La fenêtre native est opaque pour éviter les problèmes de composition ; les transparences des panneaux restent des effets internes à l’interface.

L’accueil utilise `skinview3d` : animation idle, rotation à la souris, arrêt du rendu lorsque la page est cachée et respect de la préférence de réduction des animations. Steve est disponible localement sans connexion. Après connexion, le skin actif du profil Minecraft est téléchargé par Rust uniquement depuis `https://textures.minecraft.net`, sans jeton envoyé à ce serveur ; les variantes classic/slim sont respectées. En cas d’échec, Steve reste affiché avec un statut explicite. Le remplacement par un skin de compte réel nécessite de connecter ton compte pour être validé de bout en bout.

Sur macOS, `tauri.macos.conf.json` utilise une fenêtre native opaque avec titre superposé masqué : macOS découpe les coins arrondis. Les boutons système sont masqués dans `setup` au profit des commandes du launcher. Aucun masque CSS extérieur ni transparence native ne sont nécessaires.
