# Cérémonie cut / press LIVE, runbook vidéo (deux Flex physiques, build Lot 1)

But: filmer une cérémonie complète Enclave Records entre les deux Ledger Flex,
le laptop servant de relais non fiable. Tout se pilote depuis WSL Ubuntu via
`relay/demo.py` de **ce worktree** (`presse-video`).

Build actuellement flashé sur les deux Flex:

| | |
|---|---|
| Worktree | `C:\Users\sylve\projects\presse-video` |
| Branche | `lot1-ui-polish` |
| Commit | `28c6371` + les correctifs deux-emplacements / library en liste |
| `data_size` | 17408 |
| Hash app | `c4fe77fc9f27641d…` |
| NVM | vierge des deux côtés après re-sideload (aucun master, aucun pressage) |

> Ce runbook ne vaut QUE pour ce build. Le worktree `presse-classic` porte une
> version antérieure du protocole: son `relay/demo.py` construit la trame `CUT`
> à l'ancien format et **le cut échouera** contre le Lot 1 (voir §5).

Le relais ne voit jamais de clé: chaque confirmation se fait sur l'écran du
device. C'est le sens de l'étape des 4 mots.

---

## 0. Câblage et rôles

- **Les DEUX Flex sont attachés en même temps.** `relay/demo.py` appelle
  `enumerate_ledgers()` et exige 2 chemins HID (sinon `need 2 Ledger devices`).
- `paths[0]` = **Flex A (master)**, `paths[1]` = **Flex B (receiver)**. L'ordre
  vient du **tri des chemins HID**, pas de l'ordre de branchement.
- Le sideload, lui, est un-à-la-fois. Les devices sont déjà flashés: **on ne
  sideload pas** pour la démo.

---

## 1. Pré-requis (avant de lancer la caméra)

1. Les deux Flex branchés en USB, **déverrouillés**, application **Enclave
   Records ouverte** (écran library affiché). **Ledger Live fermé** (il verrouille
   l'interface USB).
2. `usbipd-win` installé (une fois, admin: `winget install dorssel.usbipd-win`),
   chaque device **bind** une fois (admin): `usbipd list`, repérer les busid
   `2c97`, puis `usbipd bind --busid <ID>`. Helper: `scripts/bind-flex.ps1`.
3. Attacher les deux devices à WSL (à refaire à chaque session), dans PowerShell:

   ```powershell
   C:\Users\sylve\projects\presse-video\scripts\attach-usb.ps1
   ```

   La sortie finale doit lister **deux** lignes `2c97` attachées.

4. Pré-vol en **lecture seule** (aucune commande UI, aucun master consommé):

   ```powershell
   wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/preflight.sh
   ```

   Doit afficher `2 Flex vu(s) en HID`, et pour chacun `has_master: False`,
   `has_pressing: False`. L'`empreinte` imprimée est exactement celle que
   l'écran de A affichera au moment du press (`For device XXXXXXXX`): note
   laquelle est `paths[1]`, c'est le futur destinataire B.

   Si `1 Flex vu(s)`: relancer `attach-usb.ps1`, vérifier que le device est
   déverrouillé et l'app ouverte.

> `demo.py` n'utilise NI `APP_DIR` NI `APP_ELF` pour la cérémonie: il ne fait que
> du HID. Les wrappers `scripts/ceremony.sh` et `scripts/preflight.sh` mettent
> simplement le `python3` du venv (`~/venv-ledger/bin`) dans le PATH. Ne PAS
> sourcer `scripts/env.sh`: il épingle `APP_DIR` sur le checkout `../presse`.

---

## 2. Séquence commande par commande

Toutes les commandes se copient telles quelles dans **PowerShell**.
(Ne pas retaper la version `wsl -- bash -c "export PATH=…:$PATH && …"`:
PowerShell mange le `$PATH` avant que WSL ne le voie. D'où les wrappers `.sh`.)

### 2a. Cérémonie complète (cut, pair, press, verify)

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/ceremony.sh
```

Valeurs par défaut: titre `Random Access Memories`, artiste `Daft Punk`,
édition `5`. Pour changer:

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/ceremony.sh --title "Random Access Memories" --artist "Daft Punk" --edition 5
```

Limites dures du firmware: titre 1 à 32 octets UTF-8, **artiste 0 à 13 octets**,
édition >= 1. Au-delà, le device répond `WrongApduLength` et rien n'est coupé.

`demo.py` déroule seul, en bloquant à chaque étape gated jusqu'au tap physique:

1. **cut**: A n'ayant pas de master, le relais uploade la pochette
   (`docs/art/ram-cover.bin`, 2048 octets) puis envoie `INS_CUT`. A demande de
   confirmer. (Si A avait déjà un master, l'étape serait sautée et annoncée.)
2. **pairing**: commit-reveal ECDH à travers le relais, puis `INS_PAIR_SAS` sur
   les deux: les 4 mots s'affichent des deux côtés.
3. **press**: `INS_PRESS_OFFER` sur A (confirmer), `INS_PRESS_LOAD_ALBUM` sur B,
   la pochette est transportée A vers B (`carry_sleeve`), puis
   `INS_PRESS_ACCEPT` sur B (confirmer).
4. **verify**: chaîne de certs + challenge-response en direct sur B, imprime
   `GENUINE: pressing 1 of 5 of "Random Access Memories". No server, no chain,
   no trust in this laptop.`

### 2b. Finale: vérification offline seule (un seul device, wifi coupé)

Débrancher A, ne garder que **B** attaché, couper le wifi, puis:

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/ceremony.sh --verify
```

Imprime `GENUINE: pressing 1 of 5 of "Random Access Memories", bound to this
device, key possession proven live.`

`--verify` ne prend que `paths[0]`: **un seul device doit rester attaché**, et ce
doit être B. Si A est encore attaché et sort en `paths[0]`, la commande échoue
(A ne détient aucun pressing).

### 2c. (option) Parcourir la collection d'un device

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/ceremony.sh --collection a
```

(`a` = `paths[0]`, `b` = `paths[1]`.) Tap « Back » sur l'écran pour sortir.

---

## 3. Ce qui s'affiche sur chaque device, quoi filmer

Textes relevés sur le build Lot 1 (répétition Speculos, titre et artiste par
défaut). Sur l'écran, un titre long se coupe en deux lignes.

### Avant tout

Les deux Flex: `Enclave Records` / **No records yet** / « Cut a master or
receive a pressing. » / `Quit`.

### Cut, sur A

```
Cut master of
Random Access Memories by Daft Punk?

Edition of 5, fixed forever.
Losing this device destroys the plates.

[ Cancel ]   [ Cut the master ]
```

Après le tap, la library de A devient une ligne: vignette de pochette, titre,
et le statut **Master - 5 of 5 left**, au-dessus d'un pied de page `Quit`. La
liste est le seul état de la library, quel que soit le nombre de disques: la
ligne elle-même ouvre la fiche, il n'y a plus d'affordance « Open ».

**À filmer**: l'écran A, la ligne « Edition of 5, fixed forever ».

### Pair, sur les DEUX

```
blizzard   eclipse   papaya   noodle

Confirm only if the other device
shows exactly these words.

[ Abort ]   [ Words match ]
```

**À filmer**: les deux écrans côte à côte, les 4 mots identiques, dits à voix
haute, puis « Words match » tapé sur les deux. Moment clé.

### Press, sur A

```
Press Random Access Memories
1 of 5?

For device DCFE1B7F.
4 pressings will remain.

[ Cancel ]   [ Press this copy ]
```

L'empreinte affichée est celle de B (celle donnée par `preflight.sh`).
Après le tap, la library de A passe à **4 of 5 left to press**: le compteur a
bougé dans le silicium.

### Receive, sur B

```
Receive Random Access Memories
1 of 5?

This pressing is bound to
this device forever.

[ Cancel ]   [ Receive it ]
```

Après le tap, la library de B repeint: **la vraie pochette RAM** + titre +
`#1 of 5` / `Quit` `Open`. La pochette a été portée avant le `PRESS_ACCEPT`,
donc le repaint montre directement la vraie image, jamais le fallback génératif.

**À filmer**: l'écran B, la pochette qui apparaît = preuve que le pressing a
atterri.

### Verify

**À filmer**: le terminal, ligne `GENUINE: …`, wifi coupé visible à l'image.

Narration relais côté terminal, à laisser visible: `== presse: cut ==`,
`pairing (this relay is untrusted)`, `press`, `offline verification of Flex B`.

---

## 4. Format de la trame CUT (Lot 1)

Utile seulement pour diagnostiquer un refus: `demo.py` construit la trame.

```
CLA=0xB5  INS=0x10  P1=0x00  P2=0x00  Lc=len(data)

data = edition (u16 little-endian)
    || title_len (1 octet)
    || title (title_len octets UTF-8, 1..32)
    || artist (0..13 octets UTF-8, la fin de la trame)
```

Trame nominale, « Random Access Memories » par « Daft Punk », édition 5:

```
b5 10 00 00 22 05 00 16 52616e646f6d20416363657373204d656d6f72696573 446166742050756e6b
                  ^^^^^ ^^ édition 5 (LE), title_len 0x16 = 22
```

Refus possibles: `WrongApduLength` si `title_len` vaut 0 ou dépasse 32, ou si
l'artiste dépasse 13 octets; `BadCert` si l'édition vaut 0; `HasMaster`
(`0xB106`) si le device tient déjà un master.

Le plafond de 13 octets sur l'artiste n'est pas cosmétique: l'AlbumCert (223) +
MAC (32) doit tenir dans un `Lc` de 255.

---

## 5. Pièges connus

- **Ne PAS utiliser `presse-classic/relay/demo.py`.** Il envoie
  `edition(2) || title` sans octet de longueur et sans artiste. Contre le Lot 1,
  le premier octet du titre est lu comme `title_len` (`R` = 82 > 32) et le device
  répond `WrongApduLength`: **le cut échoue**. Toutes les commandes de ce runbook
  passent par `presse-video`.
- **NVM vidée = refaire le cut.** `demo.py` le gère: il ré-uploade la pochette et
  ré-exécute le cut sur A. Mais si c'est A qui a été vidé, il faut **refilmer le
  cut**. Une NVM vidée par un re-sideload implique aussi `install-ca.sh` puis
  `scripts/load-video.sh` (un seul device attaché à la fois).
- **Rôle A vs B non contrôlé par le branchement.** `paths[0]`=A, `paths[1]`=B
  vient du tri des chemins HID. Les deux devices sont vierges aujourd'hui, donc
  le cut tombera sur `paths[0]` quel qu'il soit: lancer `preflight.sh` pour
  savoir lequel c'est **avant** de cadrer la caméra.
- **`usbipd` un-à-la-fois vs les deux.** Cérémonie: les deux attachés en même
  temps (obligatoire). Sideload: un seul à la fois. Si Windows n'en expose qu'un,
  relancer `attach-usb.ps1` (il boucle sur tous les `2c97`).
- **Ledger Live doit être fermé**, sinon `enumerate_ledgers()` ne voit rien.
- **Les APDU gated bloquent sans timeout** jusqu'au tap physique. Rien ne « rate »
  si on prend son temps: les 4 mots restent affichés tant qu'on n'a pas tapé.
- **`scripts/env.sh`, `test.sh`, `emu-up.sh`, `demo-emu.sh` pointent sur le
  checkout `../presse`.** Sans effet sur la cérémonie (qui n'utilise pas
  `APP_ELF`), mais ne pas s'en servir pour juger le build Lot 1.
- **Verify offline**: ne garder que B attaché et couper le wifi à l'image.
- **Ne pas toucher au checkout `C:\Users\sylve\projects\presse`** (occupé).
- **Un pressage ne se transfère pas d'appareil à appareil.** Le certificat de
  pressage lie `recvpub` au destinataire au moment du press, et `PRESS_ACCEPT`
  refuse (`BadCert`) tout certificat dont le `recvpub` n'est pas celui du device.
  Il n'existe aucune commande de cession, et presser exige un master
  (`PRESS_OFFER` répond `NoMaster` sans lui). Un appareil ne détient par
  ailleurs qu'un seul pressage à la fois (`has_pressing` déjà à 1 -> `BadState`).
  C'est un choix de modèle, pas un manque: « bound to this device forever » est
  affiché à la réception. Ne pas construire de scène de revente ou de don.

---

## 6. Répéter sans consommer de master

La cérémonie complète tourne sur deux Speculos, avec l'ELF Lot 1 de ce
worktree, sans toucher au matériel:

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/rehearse-emu.sh --auto
```

Sans `--auto`, les deux écrans se regardent (et se tapent) dans un navigateur,
http://localhost:5001 pour A et http://localhost:5002 pour B.

Réserve: `demo_emu.py` n'uploade pas la pochette avant le cut, contrairement au
chemin matériel de `demo.py`. La répétition valide le protocole et les écrans,
pas le repaint de la pochette sur le receveur.

---

## 7. Checklist « prêt à filmer »

- [ ] Deux Flex branchés, déverrouillés, Enclave Records ouverte, Ledger Live fermé.
- [ ] `attach-usb.ps1` lancé depuis `presse-video`.
- [ ] `preflight.sh`: **2 Flex**, les deux `has_master: False`.
- [ ] Empreinte de `paths[1]` notée (elle apparaîtra à l'écran du press).
- [ ] Commande §2a prête, pointant sur `presse-video`.
- [ ] Caméra cadrée sur les deux écrans (mots SAS côte à côte) + un plan terminal.
- [ ] Wifi coupable à l'image pour la finale §2b.
