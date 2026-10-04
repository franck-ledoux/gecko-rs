# gecko-rs

[![Build & Test](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/franck-ledoux/gecko-rs/actions/workflows/ci.yml)

Portage Rust de [GECKO](https://github.com/franck-ledoux/gecko), repris pas à pas.

```bash
./check.sh          # tout, dans l'ordre qui échoue le plus vite
```

## Les deux modes

**Statique** — des fichiers à déposer sur un site. La page embarque le noyau
compilé en wasm ; aucun serveur, aucun Python.

```bash
./build-wasm.sh && cd front && npm run dev
```

**Station** — un module Python local tient le noyau et le sert à un navigateur.

```python
import gecko
s = gecko.Session()
s.create_cube([0, 0, 0], 1.0)
s.show()                       # ouvre un onglet ; la vue suit la console

n = s.nodes[0]                 # {"id": …, "pos": [x, y, z]}
s.move_node(n["id"], [0.5, -0.2, 0])
```

Dans l'onglet, les sommets se **tirent à la souris**, et la session Python voit
le résultat — c'est le même objet.

## Ce qui rend les deux modes possibles

Le front TypeScript + Babylon.js est **le même dans les deux cas**. Seule change
la provenance des données :

```
front/src/kernel.ts          l'interface : apply, view, stats, onChanged
front/src/kernel-wasm.ts     ← statique : le noyau que la page possède
front/src/kernel-remote.ts   ← station  : le noyau que tient Python
front/src/viewer.ts  main.ts          identiques
```

Et sous les deux, une seule porte : `gecko_cmd::Session`. Les façades wasm et
PyO3 traduisent, rien de plus — `gecko-wasm/src/lib.rs` et `gecko-py/src/lib.rs`
se lisent côte à côte. C'est cette règle qui rend le `test_modes.py` possible :
rejouer une séquence des deux côtés doit donner les **mêmes octets**.

Trois choses à savoir, parce qu'elles sont pénibles à rattraper après coup :

| | Pourquoi |
|---|---|
| L'interface du front est **asynchrone partout**, même en statique où la réponse est immédiate | Sinon tous les appels supposeraient une réponse synchrone, et la station ne pourrait s'ajouter qu'en les réécrivant tous. |
| `onChanged` existe dès le départ | En station, le noyau bouge sans que l'onglet ait rien demandé : quelqu'un a tapé dans la console. Un client qui ne scrute qu'après ses propres commandes ne le verrait pas. |
| Les positions partent en **f32 recentrées**, le décalage f64 à côté | WebGL n'a que du f32 ; à 10⁶ unités de l'origine, le pas f32 vaut 0,06 — gigue visible et z-fighting. |

En station, la `Session` vit dans un `Arc<Mutex<…>>` partagé entre la poignée
Python et les tâches du serveur. Ce n'est pas un détail : c'est ce qui rend
littéralement vrai « la console et la souris pilotent le même objet vivant ». Le
lien est bidirectionnel, et plusieurs onglets voient le travail les uns des
autres.

## Le noyau

La structure de blocs repose sur une **3-carte combinatoire**
([`honeycomb-core`](https://github.com/LIHPC-Computational-Geometry/honeycomb)),
le pendant Rust de ce que `CGAL::Combinatorial_map` fournit à la version C++ :
darts, attributs aux quatre dimensions, functors `merge`/`split`.

Deux contraintes l'ont façonné :

1. **`AttributeUpdate` exige `Copy`.** Rien de taille variable ne peut vivre dans
   un attribut. L'attribut ne porte donc qu'un **nom stable**, et tout ce qui
   grossit pendra à côté, dans des tables indexées par ce nom.
2. **Les identifiants de cellules sont dérivés des darts**, donc réécrits par
   chaque édition. Inutilisables comme nom à conserver.

D'où les deux identités de `ids.rs`, reprises telles quelles de `CellData.h` :
un `CellId` stable jamais réutilisé, *effacé* au split, et un `CornerKey` qui
*survit* au split parce qu'une orbite coupée en deux reste le même coin pour
toutes les cellules qui l'avaient.

## La pyramide de tests

| Étage | Outil | Durée |
|---|---|---|
| Noyau et porte | `cargo test` | ~0,05 s |
| Décodage, scène, transport | Vitest + **`NullEngine`** | ~0,3 s |
| Les deux modes dans un navigateur | Playwright + SwiftShader | ~6 s |
| Façade Python, et son accord avec le mode statique | pytest | ~0,05 s |

`NullEngine` est la pièce non évidente : un graphe de scène Babylon complet sans
contexte de rendu, donc décodage, construction de scène et caméra se testent en
millisecondes dans Node. Playwright ne garde que ce qui exige un navigateur —
dont le test qui justifie le mode station : **une commande tapée en Python
atteint la page sans que la page ait rien demandé.**

## Nommer un sommet

Un sommet se désigne par son **`CellId` stable**, jamais par l'identifiant que
la carte dérive des darts : celui-là est réécrit par chaque édition, donc un
appelant qui le garderait entre deux opérations désignerait autre chose. Le
tampon de vue transporte donc ces noms, sans quoi le navigateur pourrait piquer
une sphère sans rien avoir à renvoyer.

La résolution nom → cellule est un **parcours**, pas un index en cache : un index
devrait être invalidé par chaque changement de topologie, et se tromper là est
silencieux.

Pendant un glisser-déposer, la vue **prévisualise en local** et le noyau n'est
prévenu qu'au lâcher, par une commande unique — sinon c'est un aller-retour
réseau par mouvement de souris en mode station. La vue diverge donc du noyau le
temps du geste, puis est reconstruite depuis lui, qui reste l'autorité. Et parce
que le fil est en f32 alors que le noyau calcule en f64, un aller-retour par le
tampon est volontairement imprécis : c'est le noyau qui fait foi.

## Ce qui n'est pas encore là

Le noyau sait créer un cube et déplacer un sommet. Pas d'annulation, pas de
validité géométrique (rien n'empêche de retourner un bloc), pas de
classification, pas de courbes de Bézier ni de surfaces de Coons, aucune autre
opération topologique (coudre, couper une nappe, supprimer, pillower), pas de
maillage, pas de lecture de fichier.

Le prochain choix de conception est le stockage de la classification : tableau de
taille fixe dans l'attribut, ou table latérale indexée par le `CellId`.

## Licence

AGPL-3.0-only, comme GECKO — voir [LICENSE](LICENSE). Les dépendances sont sous
MIT ou Apache-2.0, donc compatibles.

L'article 13 n'est pas décoratif ici : le mode station **sert l'IHM par HTTP**,
donc quiconque déploie une version modifiée doit en proposer les sources à ceux
qui s'y connectent.

## Notes d'outillage

`check.sh` s'amorce tout seul — venv, `npm install`, cible wasm32 — pour qu'un
clone neuf n'ait besoin que de Rust, Node et Python. **La CI exécute ce même
script** : ce qui passe en local passe là-bas.

`build-wasm.sh` passe par **wasm-pack**, qui lit la version de `wasm-bindgen`
dans le `Cargo.lock` et installe le CLI correspondant. Piloter `cargo` et
`wasm-bindgen` séparément oblige à tenir ces deux versions synchronisées à la
main, et elles dérivent en silence : la glue générée cesse simplement de
correspondre au module. `wasm-opt` tourne au passage (272 Ko au lieu de 356).

L'ouverture du navigateur est faite à la main (`open` / `xdg-open` / `start`)
plutôt qu'avec le crate `webbrowser` : celui-ci tire **39 dépendances** — `url`,
`idna`, toute la famille `icu` — pour lancer une commande. Mesuré.
