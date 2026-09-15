# hyperx-overlay

Petit overlay Windows (toujours au premier plan, sans bordure) qui affiche :

- l'état de la touche **Verr. Maj**
- le niveau de batterie de la souris **HyperX Pulsefire Saga Pro** (sans fil)
- le niveau de batterie du casque **HyperX Cloud III S Wireless**

## Fonctionnement

- Le Verr. Maj est lu directement via l'API Win32 (`GetKeyState`), rafraîchi en continu.
- Les niveaux de batterie sont lus en parlant directement en HID aux dongles USB
  sans fil, avec le protocole propriétaire rétro-ingénieré par la communauté
  (HyperX/HP ne documente pas ce protocole) :
  - Pulsefire Saga Pro : [notwaterbtl/hyperx-saga-control](https://github.com/notwaterbtl/hyperx-saga-control)
  - Cloud III S Wireless : [auto94/HyperX-Cloud-2-Battery-Monitor](https://github.com/auto94/HyperX-Cloud-2-Battery-Monitor)

  Si un appareil n'est pas détecté, l'overlay affiche `--`.
  Pour la souris, si le récepteur reste présent mais ne répond plus, l'overlay
  affiche la dernière batterie connue avec `Veille` en gris. Cet état est une
  estimation : une souris éteinte ou hors de portée peut produire le même résultat.

- Lecture adaptative : souris en charge toutes les 10 s, active sur batterie
  toutes les 15 s, en veille toutes les 60 s ; casque toutes les 30 s.
  Les délais s'ajoutent au temps des requêtes et des éventuels réessais HID.
  Le réveil est reconnu à la prochaine lecture réussie.
- Batterie : vert en charge, blanc/gris clair autrement, orange à 30 % ou moins,
  rouge à 15 % ou moins. La détection de charge du casque n'est pas disponible.
- Le tray dispose d'un thread et d'une boucle de messages Win32 dédiés.

- Fenêtre déplaçable : cliquer-glisser n'importe où dessus pour la repositionner.
- `Alt + Clic` sur l'overlay : le masque (il reste accessible depuis le tray).
- Icône dans la zone de notification (tray), toujours présente :
  - **Double-clic** : affiche/masque l'overlay.
  - **Clic droit** : menu avec *Afficher/Masquer l'overlay*, *Lancer au
    démarrage* (ajoute/retire l'app de `HKCU\...\Run`, sans droits admin) et
    *Quitter*.
- `Échap` (fenêtre active) ferme complètement l'overlay.

## Build

```
cargo build --release
```

L'exécutable est autonome (`target/release/hyperx-overlay.exe`), aucune DLL
externe requise (le crate `hidapi` utilise l'implémentation HID native de
Windows via la feature `windows-native`).

## CI

Le workflow GitHub Actions (`.github/workflows/build.yml`) build l'exécutable
sur `windows-latest` à chaque push, et publie `hyperx-overlay.exe` en artefact.
Un tag `vX.Y.Z` crée en plus une Release GitHub avec l'exe attaché.

## Limites connues

Protocole non officiel : HyperX/HP peut le changer à tout moment via une mise
à jour firmware/NGENUITY, ce qui casserait la lecture de batterie (le Verr.
Maj continuerait de fonctionner). Écrit et compilé sans matériel HyperX
disponible pour tester — à valider sur une vraie Pulsefire Saga Pro / Cloud
III S.

## Si la batterie n'affiche rien (`--`)

Lancer l'overlay avec la variable d'environnement `HYPERX_OVERLAY_DEBUG=1`
depuis un terminal (pas en double-cliquant) pour activer un journal détaillé :

```powershell
$env:HYPERX_OVERLAY_DEBUG = "1"
.\hyperx-overlay.exe
```

Laisser tourner quelques minutes (les intervalles dépendent de l'état de la
souris), fermer avec `Échap`, puis envoyer le fichier généré :

```
%TEMP%\hyperx-overlay-debug.log
```

Ce fichier liste tous les périphériques HID HyperX/Kingston détectés (VID,
PID, usage page, nom) ainsi que chaque requête/réponse brute échangée avec la
souris et le casque — de quoi corriger précisément les identifiants ou le
format de trame sans avoir le matériel sous la main.
