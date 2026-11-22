# Où PixStride 0.5.0 prend du retard

Sur les 1 000 recettes de la suite V2, **657 produisent un rendu comparable avec au moins un des cinq serveurs sur AMD64 et ARM64**. En mode par défaut, **46 recettes** coûtent à PixStride au moins 10 % de CPU et 10 % de latence médiane supplémentaires face au même serveur sur les deux architectures. **23 de ces 46 recettes** étaient des victoires de PixStride sur les quatre métriques de performance face à imgproxy dans la campagne publiée, sur les deux architectures.

Le seuil s’applique séparément au meilleur CPU et à la meilleure p50 des trois répétitions historiques de PixStride. Les 46 recettes restent en retard en mode natif forcé face aux mêmes serveurs. Pour 33 d’entre elles, les deux architectures disposent aussi d’au moins une observation de PixStride correspondant au modèle de CPU du concurrent.

## Trois opérations à examiner en premier

| Opération et cas | Serveur | p50 AMD64, PixStride / concurrent | p50 ARM64, PixStride / concurrent | Taille, PixStride / concurrent |
|---|---|---:|---:|---:|
| PNG transparent 3 072 × 4 096 → redimensionnement, flou, accentuation et JPEG, cas 0904 | IPX | 361,50 / 225,55 ms | 490,26 / 312,29 ms | 98 807 / 49 956 octets |
| TIFF → ajustement sur canevas PNG, cas 0805 | weserv | 133,50 / 108,20 ms | 165,08 / 139,64 ms | 287 610 / 287 398 octets |
| GIF statique → recadrage fractionnel et redimensionnement, cas 0824 | weserv | 133,16 / 108,44 ms | 141,51 / 103,52 ms | 427 342 / 400 368 octets |

Les p50 de PixStride dans ce tableau sont les médianes des observations historiques du même modèle de CPU. Le concurrent dispose de 144 requêtes chronométrées dans une seule exécution par cas et architecture.

- **Flou suivi de l’accentuation.** Le retard revient sur 16 recettes distinctes, face à IPX ou Thumbor. IPX devance PixStride sur les dix tailles de l’entrée PNG à palette. Sur le cas 0937, PixStride demande 747 ms au p50 ARM64 contre 290 ms pour IPX, mais produit 1,87 Mo contre 2,78 Mo et utilise moins de la moitié de la mémoire. Le cas transparent 0904 est une meilleure cible initiale pour une optimisation conjointe du temps, de la mémoire et du poids.
- **Placement sur canevas.** weserv devance PixStride sur neuf recettes, dont sept avec une source TIFF. Sur le cas 0805, les deux serveurs reproduisent les pixels visibles de référence sur les deux architectures. Le pic mémoire AMD64 est de 74,5 Mio pour PixStride contre 54,2 Mio pour weserv. L’écart de taille se limite à 212 octets.
- **Recadrage fractionnel.** Le retard revient sur dix recettes distinctes, neuf face à weserv et une face à Thumbor. Les sources sont des GIF statiques ou des JPEG progressifs. Pour le GIF du cas 0824, PixStride consomme aussi plus de mémoire et envoie 6,7 % d’octets supplémentaires.
- **Retournement de PNG transparents.** IPX devance PixStride sur cinq tailles. Les PNG de PixStride sont tous plus petits sur ces cinq cas. Le poids de sortie aide à interpréter le gain de temps que pourrait apporter un autre réglage d’encodage.

Neuf recettes cumulent le retard CPU/p50 défini ci-dessus avec une p95, une empreinte mémoire et un poids de sortie au moins aussi élevés pour PixStride sur **chacune** des deux architectures et pour les trois références historiques. Huit concernent weserv (six canevas TIFF et deux recadrages de GIF), la neuvième est le cas 0904 avec IPX.

## Couverture et résultats par serveur

| Serveur | Cas comparables sur les deux architectures | Retards CPU + p50 sur AMD64 | Sur ARM64 | Sur les deux | Autre écart sur les deux architectures |
|---|---:|---:|---:|---:|---|
| weserv/images 5.1.2 | 572 | 42 | 51 | **22** | Sur 37 cas, le pic mémoire de PixStride est supérieur d’au moins 20 %. |
| IPX 4.1.0 | 542 | 27 | 26 | **19** | Sur 77 cas, le fichier de PixStride est au moins 20 % plus lourd. |
| Thumbor 7.8.0 | 417 | 9 | 18 | **7** | Sur 76 cas, le fichier de PixStride est au moins 20 % plus lourd. |
| Imageflow.Server 0.9.0 | 214 | 5 | 0 | **0** | Imageflow produit des fichiers plus légers d’au moins un facteur 1,2 sur 53 cas, dont 52 JPEG. |
| Imaginary 1.2.4 | 319 | 1 | 0 | **0** | Sur 46 cas, le fichier de PixStride est au moins 20 % plus lourd. |

Les 48 couples recette/serveur en retard sur les deux architectures correspondent à 46 recettes distinctes. Les colonnes par architecture incluent les résultats comparables sur cette architecture. Le poids se compare uniquement pour des rendus dans les seuils numériques de la suite. Les poids et les temps observés incluent les choix de codec, d’effort d’encodage et de traitement propres à chaque produit.

## Données et vérification

Les cinq serveurs ont été confrontés chacun aux 1 000 recettes sur deux architectures, soit 10 000 entrées. **4 131** ont passé les contrôles et livré les 144 réponses chronométrées, soit **594 864 requêtes retenues**. Parmi les autres entrées, 4 860 n’avaient pas de traduction dans l’adaptateur, 980 dépassaient les seuils de pixels et 29 ont rencontré une erreur. Le CSV contient les 10 000 entrées avec leurs motifs. Une recette non traduite décrit la version et l’adaptateur essayés.

Les trois répétitions de PixStride et d’imgproxy proviennent du benchmark V2.3 déjà publié. Les cinq nouveaux serveurs ont chacun une répétition par cas et architecture sur des VM GitHub distinctes. Les écarts modestes restent sensibles au matériel et à la variation entre VM, en particulier sur AMD64. La comparaison couvre le service HTTP complet, ainsi que la lecture locale, la transformation et l’encodage. [Protocole](../../peers/README.md) · [Référence publiée](https://github.com/lansbe/pixstride/actions/runs/44920819863)

La première campagne a construit les cinq serveurs. L’adaptateur Imageflow a ensuite été aligné sur le rééchantillonnage Lanczos en sRGB de la référence. Les 2 000 entrées Imageflow du rapport viennent intégralement de cette seconde campagne. Les échantillons de référence ont été récupérés une fois hors chronométrage et vérifiés par SHA-256. [Campagne des cinq serveurs](https://github.com/lansbe/pixstride/actions/runs/45043647940) · [Campagne Imageflow retenue](https://github.com/lansbe/pixstride/actions/runs/45043787548) · [Vérification des images de référence](https://github.com/lansbe/pixstride/actions/runs/45043123579)

Le tableau interactif permet de filtrer les retards reproduits sur les deux architectures. Il s’ouvre localement dans un navigateur. [Tableau interactif](comparaison.html) · [Comparaisons CSV](comparaison.csv) · [100 journaux bruts compressés](echantillons.tar.gz)

Pour régénérer les résultats depuis la racine du dépôt, avec Python 3.12 et Pillow 13.0.0 :

```sh
mkdir -p /tmp/pixstride-peer-results
tar -xzf benchmarks/results/peers/echantillons.tar.gz -C /tmp/pixstride-peer-results
python3 scripts/benchmark_peer_report.py --csv comparaison.csv /tmp/pixstride-peer-results/*.jsonl
python3 scripts/benchmark_peer_dashboard.py comparaison.csv --output comparaison.html
```
