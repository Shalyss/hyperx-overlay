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

  Si un appareil n'est pas détecté ou ne répond pas, l'overlay affiche `--`
  au lieu de planter.

- Fenêtre déplaçable : cliquer-glisser n'importe où dessus pour la repositionner.
- `Échap` (fenêtre active) ferme l'overlay.

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
Maj continuerait de fonctionner). Testé/écrit sans matériel HyperX branché au
moment du développement initial — à valider une fois les dongles connectés.
