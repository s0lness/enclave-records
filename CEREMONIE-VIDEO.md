# Cérémonie cut / press LIVE, runbook vidéo (deux Flex physiques)

But: filmer une cérémonie complète Enclave Records entre les deux Ledger Flex,
le laptop servant de relais non fiable. Tout se pilote depuis WSL Ubuntu via
`relay/demo.py` de **ce worktree** (`presse-video`).

> Ce runbook décrit un poste **Windows + WSL**: les chemins absolus et les deux
> `.ps1` de `scripts/windows/` ne servent qu'à faire traverser l'USB jusqu'à WSL.
> Sur Linux ou macOS, sauter §1.2 et §1.3 (le Flex est déjà visible) et lancer
> directement `scripts/preflight.sh`, `scripts/ceremony.sh`, `scripts/give.sh`
> depuis la racine du dépôt. Le reste du runbook est identique.

Build actuellement flashé sur les deux Flex:

| | |
|---|---|
| Worktree | `C:\Users\sylve\projects\presse-video` |
| Branche | `master` |
| Commit source | `d27335c` (dernier commit touchant `device-app/`; les suivants ne changent que la doc) |
| `text` | 74032 (plancher de la fenêtre de boot 74032..76080) |
| `data_size` | 18944 |
| Hash app | `a0f7870f35fbb82c…` (`device-app/target/flex/release/presse.sha256`) |
| NVM | **à vérifier au pré-vol**: le tournage précédent a laissé un master et une copie sur les appareils |

> Ce runbook ne vaut QUE pour ce build. Le worktree `presse-classic` porte une
> version antérieure du protocole: son `relay/demo.py` construit la trame `CUT`
> à l'ancien format et **le cut échouera** contre celui-ci (voir §5).

> Ce que porte le build flashé: la clé porteuse (une copie est liée à une clé,
> pas à un appareil), la cession complète `INS_GIVE_*`/`INS_TAKE_*` avec son
> annulation, le verso de fiche à quatre lignes (Number, Edition ID, Device ID,
> Learn more) et la sous-page provenance. §2a, §2b et §2c sont donc tous
> filmables sans reflasher.
>
> `data_size` 18944 est **à une page du plafond mesuré**: l'app cesse de démarrer
> quelque part entre 18432 et 19456. Le boot check passe trois fois sur ce build,
> mais toute nouvelle croissance de `.nvm_data` doit être re-vérifiée avant
> sideload, sinon l'app s'installe et meurt sans un mot (`AGENTS.md` donne la
> fenêtre complète et la procédure).
>
> Reproduire exactement ce binaire: `scripts/build-video.sh`, puis comparer
> `device-app/target/flex/release/presse.sha256` au hash du tableau.

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
   `2c97`, puis `usbipd bind --busid <ID>`. Helper: `scripts/windows/bind-flex.ps1`.
3. Attacher les deux devices à WSL (à refaire à chaque session), dans PowerShell:

   ```powershell
   C:\Users\sylve\projects\presse-video\scripts\windows\attach-usb.ps1
   ```

   La sortie finale doit lister **deux** lignes `2c97` attachées.

4. Pré-vol en **lecture seule** (aucune commande UI, aucun master consommé):

   ```powershell
   wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/preflight.sh
   ```

   Doit afficher `2 Flex vu(s) en HID`. `has_master` et `has_pressing` disent
   l'état laissé par la prise précédente: **pour refilmer le cut il faut une NVM
   vierge**, donc un re-sideload (`install-ca.sh` puis `scripts/load-video.sh`,
   un seul device attaché à la fois). Si A tient déjà un master, `demo.py` saute
   le cut et l'annonce, et §2b comme §2c restent filmables tels quels.
   Le `Device ID` imprimé est exactement celui que
   l'écran de A affichera au moment du press (`For device XXXXXXXX`), et celui
   que chaque Flex montre lui-même (au verso de la fiche, ou sous le message
   d'une library vide): note lequel est `paths[1]`, c'est le futur destinataire B.

   Si `1 Flex vu(s)`: relancer `attach-usb.ps1`, vérifier que le device est
   déverrouillé et l'app ouverte.

> `demo.py` n'utilise NI `APP_DIR` NI `APP_ELF` pour la cérémonie: il ne fait que
> du HID. Les wrappers `scripts/ceremony.sh` et `scripts/preflight.sh` sourcent
> `scripts/env.sh`, qui déduit la racine du dépôt de sa propre position et met le
> `python3` du venv (`~/venv-ledger/bin`) dans le PATH: ils agissent donc sur le
> checkout où ils vivent, sans chemin absolu.

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

Imprime `GENUINE: pressing 1 of 5 of "Random Access Memories", held by this
device, key possession proven live.`

`--verify` ne prend que `paths[0]`: **un seul device doit rester attaché**, et ce
doit être B. Si A est encore attaché et sort en `paths[0]`, la commande échoue
(A ne détient aucun pressing).

### 2c. Cession: B redonne son pressage à A

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/give.sh
```

`paths[0]` donne à `paths[1]`; `--swap` inverse. Le pressage étant lié à une
**clé porteuse** et non à un appareil, il se cède un nombre illimité de fois.
Même appairage 4 mots que le press, puis deux confirmations, **le receveur
d'abord**:

```
1. Receive Random Access Memories   |  2. Give Random Access Memories
   #1 of 5?                         |     #1 of 5?
                                    |
   From device 3FC2A9B1.            |     To device DCFE1B7F.
                                    |     You will no longer hold it.
                                    |
   [ Cancel ]  [ Receive it ]       |     [ Cancel ]  [ Give it away ]
```

**À filmer**: les deux taps dans cet ordre, l'écran du donneur qui passe à
`No records here` / « You gave your copy away. », et celui du receveur où la
pochette apparaît. Puis la fiche du receveur, page 2, ligne `Device ID`
(tapée): la sous-page `Where it came from` nomme l'appareil cédant
(`XXXXXXXX, the one handover this device can prove.`). La provenance est là et
pas sur la page 2: celle-ci tient quatre lignes, pas cinq.

> **Une cession interrompue se reprend, elle ne perd rien.** Le donneur ne
> supprime pas au moment de livrer la clé: il s'*engage* envers ce receveur-là
> (sa ligne library passe à `#1 of 5 - promised, reconnect XXXXXXXX`, qui nomme
> le receveur engagé, et `INS_CHALLENGE` répond déjà `NoPressing`). Ce `XXXXXXXX`
> est le même empreinte que l'écran de confirmation (`To device XXXXXXXX`) et que
> le `Device ID` de l'appareil concerné: c'est ce qui permet de retrouver
> avec quel Flex terminer. Relancer `give.sh` sur les deux mêmes Flex termine la
> cession, **sans redemander de confirmation au donneur**. L'effacement réel
> n'a lieu qu'à la réception du reçu du destinataire.
>
> Le receveur est interrogé en premier, donc un refus, un appareil déjà occupé
> ou un certificat invalide ne coûtent rien: le donneur n'a pas encore été
> sollicité.
>
> Tant que la clé n'est pas partie, `give.sh --cancel` reprend la promesse (un
> seul Flex attaché, confirmation sur son écran). Une fois la clé envoyée, seul
> le reçu du destinataire libère le donneur: un appareil engagé envers un
> receveur qui ne revient jamais garde un pressage inutilisable pour toujours, et
> l'annulation y est refusée (`0xB10A`) parce qu'elle serait exactement une
> primitive de double dépense.

### 2d. (option) Parcourir la collection d'un device

```powershell
wsl -d Ubuntu -- bash /mnt/c/Users/sylve/projects/presse-video/scripts/ceremony.sh --collection a
```

(`a` = `paths[0]`, `b` = `paths[1]`.) Tap « Back » sur l'écran pour sortir.

> `give.sh` suit le même patron que `ceremony.sh` (`env.sh` pour le PATH du venv
> et la racine du dépôt), et ne touche ni `APP_DIR` ni `APP_ELF`.

---

## 3. Ce qui s'affiche sur chaque device, quoi filmer

Textes relevés sur le build flashé (répétition Speculos, titre et artiste par
défaut). Sur l'écran, un titre long se coupe en deux lignes.

### Avant tout

Les deux Flex: `Enclave Records` / **No records yet** / « Cut a master or
receive a pressing. » / `Device ID XXXXXXXX` / `Quit`. Une library vide nomme
l'appareil, seule façon d'identifier un Flex qui ne tient rien.

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

L'empreinte affichée est le `Device ID` de B (celui donné par `preflight.sh`).
Après le tap, la library de A passe à **4 of 5 left to press**: le compteur a
bougé dans le silicium.

### Receive, sur B

```
Receive Random Access Memories
1 of 5?

This copy becomes yours to
keep or to hand on.

[ Cancel ]   [ Receive it ]
```

Après le tap, la library de B repeint: **la vraie pochette RAM** + titre +
`#1 of 5`, au-dessus d'un pied de page `Quit` seul (la ligne elle-même est la
cible tactile, il n'y a pas de bouton `Open`). La pochette a été portée avant le
`PRESS_ACCEPT`, donc le repaint montre directement la vraie image, jamais le
fallback génératif.

**À filmer**: l'écran B, la pochette qui apparaît = preuve que le pressing a
atterri.

### La fiche du disque, sur B

Taper la ligne de la library ouvre la fiche, en deux pages (pager `< 1 of 2 >`
et `Back` en pied de page):

- **page 1**: le grand `#1`, la pochette 160px avec son reflet, le titre en gras
  et l'artiste dessous;
- **page 2**: quatre lignes navigables, chacune ouvrant sa sous-page: `Number`
  (`#1 of 5`), `Edition ID` (8 hex de `SHA256(albpub)`, la même sur toutes les
  copies de l'édition), `Device ID` (8 hex de `SHA256(devpub)`, l'appareil qui la
  tient) et `Learn more`. Quatre lignes toujours, quoi que tienne l'appareil.

**À filmer**: la page 2, puis la sous-page `Device ID` (elle porte la provenance
après une cession, voir §2c) et `Learn more`, qui dit ce que l'appareil prouve et
ce qu'il ne prouve pas.

### Verify

**À filmer**: le terminal, ligne `GENUINE: …`, wifi coupé visible à l'image.

Narration relais côté terminal, à laisser visible: `== presse: cut ==`,
`pairing (this relay is untrusted)`, `press`, `offline verification of Flex B`.

---

## 4. Format de la trame CUT (build courant)

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

Le plafond de 13 octets sur l'artiste vient d'une contrainte de trame:
l'AlbumCert (223) + MAC (32) doit tenir dans un `Lc` de 255.

---

## 5. Pièges connus

- **Ne PAS utiliser `presse-classic/relay/demo.py`.** Il envoie
  `edition(2) || title` sans octet de longueur et sans artiste. Contre ce build,
  le premier octet du titre est lu comme `title_len` (`R` = 82 > 32) et le device
  répond `WrongApduLength`: **le cut échoue**. Toutes les commandes de ce runbook
  passent par `presse-video`.
- **NVM vidée = refaire le cut.** `demo.py` le gère: il ré-uploade la pochette et
  ré-exécute le cut sur A. Mais si c'est A qui a été vidé, il faut **refilmer le
  cut**. Une NVM vidée par un re-sideload implique aussi `install-ca.sh` puis
  `scripts/load-video.sh` (un seul device attaché à la fois).
- **Rôle A vs B non contrôlé par le branchement.** `paths[0]`=A, `paths[1]`=B
  vient du tri des chemins HID, pas de l'ordre de branchement: lancer
  `preflight.sh` pour savoir lequel est lequel, et lequel tient déjà quoi,
  **avant** de cadrer la caméra.
- **`usbipd` un-à-la-fois vs les deux.** Cérémonie: les deux attachés en même
  temps (obligatoire). Sideload: un seul à la fois. Si Windows n'en expose qu'un,
  relancer `attach-usb.ps1` (il boucle sur tous les `2c97`).
- **Ledger Live doit être fermé**, sinon `enumerate_ledgers()` ne voit rien.
- **Les APDU gated bloquent sans timeout** jusqu'au tap physique. Rien ne « rate »
  si on prend son temps: les 4 mots restent affichés tant qu'on n'a pas tapé.
- **`scripts/env.sh` déduit la racine du dépôt de sa propre position.** Tous les
  scripts qui le sourcent (`build.sh`, `load.sh`, `test.sh`, `emu-up.sh`,
  `boottest.sh`, `cockpit.sh`…) agissent donc sur CE checkout, plus sur le
  sibling `../presse`. `APP_DIR`, `APP_ELF` et `FLEX_SDK` restent des défauts
  surchargeables: exporter l'un d'eux avant l'appel gagne.
- **Verify offline**: ne garder que B attaché et couper le wifi à l'image.
- **Ne pas toucher au checkout `C:\Users\sylve\projects\presse`** (occupé).
- **Un pressage se cède, et une cession interrompue se reprend.** Le certificat
  lie le pressage à une clé porteuse (`holderpub`). `GIVE_OFFER` n'efface pas: il
  engage le donneur envers UN destinataire nommé. Relancer `give.sh` sur les deux
  mêmes Flex termine la cession; viser un autre destinataire répond `BadState`
  définitivement. Une fois le reçu consommé (`GIVE_FINISH`), `GIVE_ALBUM` et
  `GIVE_OFFER` répondent `NoPressing`.
- **`GIVE_OFFER` se joue en deux APDU** (`P1=0` la promesse, sous tap; `P1=1` la
  livraison de la clé scellée). `give.sh` enchaîne les deux, rien à faire de plus
  à l'écran. Ce qui change à l'image: entre les deux, la promesse est encore
  reprenable, et **seulement là**.
- **`give.sh --cancel` reprend une promesse dont la clé n'est jamais partie.** Un
  seul Flex attaché suffit, la confirmation est sur son écran. Refusé (`0xB10A`)
  dès que la clé a été envoyée: à partir de là, seul le reçu du destinataire
  libère le donneur. Utile si une prise se termine sur un donneur engagé.
- **Un appareil ne détient qu'un seul pressage à la fois.** Le receveur étant
  interrogé en premier (`TAKE_CONFIRM`), un `BadState` de sa part ne coûte rien:
  le donneur détient toujours son pressage, non engagé.
- **La preuve de possession suit la clé, pas l'appareil.** `INS_CHALLENGE` signe
  avec la clé porteuse et répond `NoPressing` (`0xB108`) sur un appareil qui n'en
  détient aucun **ou dont le pressage est engagé**: après une cession, `--verify`
  ne marche que sur le receveur, et il ne marche déjà plus sur un donneur engagé.

---

## 6. Répéter sans consommer de master

La cérémonie complète tourne sur deux Speculos, avec l'ELF de ce worktree, sans
toucher au matériel:

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
- [ ] `preflight.sh`: **2 Flex**, et `has_master`/`has_pressing` notés pour
      chacun (les deux à `False` si le cut doit être refilmé, donc après
      re-sideload).
- [ ] Empreinte de `paths[1]` notée (elle apparaîtra à l'écran du press).
- [ ] Commande §2a prête, pointant sur `presse-video`.
- [ ] Caméra cadrée sur les deux écrans (mots SAS côte à côte) + un plan terminal.
- [ ] Wifi coupable à l'image pour la finale §2b.
