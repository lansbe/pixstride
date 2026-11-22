# Comparaison avec cinq serveurs d’images

Cette campagne compare les sorties et les performances de Thumbor 7.8.0, Imaginary 1.2.4, weserv/images 5.1.2, Imageflow.Server 0.9.0 et IPX 4.1.0 à PixStride 0.5.0. Elle reprend les 1 000 recettes de [la suite V2](../../BENCHMARKS_V2.md) et ses 120 fichiers d’entrée. Les chronométrages de PixStride et d’imgproxy viennent des trois répétitions publiées sur chaque architecture. Le [catalogue](../cases-v2.json) fixe les dimensions, les formats et les seuils d’écart de pixels avant la mesure.

[Résultats par opération et par serveur](../results/peers/RESULTATS.md) · [Tableau interactif](../results/peers/comparaison.html)

## Mesures

Sur chaque architecture, l’adaptateur traduit chaque recette dans l’API publique du serveur. Les recettes sans traduction fidèle gardent un motif. Le client vérifie le statut HTTP, décode l’image et contrôle format, dimensions, couleur et transparence. Il compare le rendu sur fond noir, sur fond blanc et sur le canal alpha. Pour récupérer les pixels de référence, absents des archives, un processus imgproxy séparé a rendu chaque image une fois hors chronométrage. Chaque empreinte SHA-256 correspond à une sortie archivée de la même architecture. Les écarts de qualité et les erreurs restent dans le bilan.

Chaque cas comparable utilise un nouveau conteneur, deux CPU, 1 Gio de mémoire, une source locale en lecture seule et un client sur des CPU distincts. Après vérification initiale et cinq requêtes de chauffe, le client relève six lots de 24 requêtes HTTP séquentielles avec une connexion neuve à chaque requête. Les empreintes des sorties chronométrées doivent rester stables. Le CPU est la médiane du temps CPU du conteneur par requête dans les six lots. La mémoire est le pic du cgroup depuis le démarrage du conteneur. Les tailles en octets portent sur l’image encodée.

Les cinq serveurs utilisent chacun leur interface locale, sans cache de résultats. Leur authentification et leur coût de lecture peuvent différer de ceux de PixStride, dont la campagne publiée vérifie la signature et le jeton à chaque requête. Imaginary utilise le cache interne d’opérations par défaut de bimg. Les paramètres de qualité portant le même nombre peuvent produire des tailles et des rendus différents.

## Lecture des résultats

Les mesures des cinq serveurs proviennent d’une campagne indépendante, avec une répétition par cas et architecture. Pour les ratios habituels, le CSV compare le pair à la médiane des observations historiques du même modèle de CPU si elles existent, sinon à la médiane de l’architecture. La colonne `same_cpu_runs` donne l’effectif correspondant. Sur AMD64, plusieurs modèles Intel et AMD apparaissent sous la même étiquette de runner GitHub. Sur ARM64, le modèle est identifié par le champ CPU part de Linux.

Un ratio `default_cpu_ratio` de 1,50 indique que PixStride en mode par défaut a consommé 50 % de CPU de plus par requête. Les colonnes `*_min_ratio` utilisent séparément la meilleure des trois mesures historiques de PixStride pour chaque métrique. Les modes par défaut et natif sont disponibles. Les écarts les plus crédibles reviennent sur plusieurs cas, notamment sur les deux architectures, avec un rendu conforme. Les seuils de pixels n’établissent pas une égalité perceptuelle entre deux encodages.

## Reproduire un essai

Sur un hôte Linux natif doté de Docker, de Python 3.12 avec Pillow 13.0.0 et d’au moins trois CPU disponibles, les commandes suivantes prennent IPX et le premier des dix groupes de cas comme exemple. Les paramètres de compilation et les dépendances des cinq serveurs se trouvent dans leurs sous-répertoires.

```sh
# corpus/ contient l’archive benchmark-v2-shared-corpus de la campagne publiée.
# À défaut, régénérer le corpus dont le manifeste sera vérifié contre l’archive.
python3 scripts/benchmark_v2_fixtures.py --output corpus --cache sources

# pixels/ contient peer-reference-amd64 ou peer-reference-arm64 selon la machine.
# Si cet artefact est indisponible, récupérer une seule fois les sorties vérifiées.
python3 scripts/benchmark_peer_baseline.py --fixtures corpus --output pixels

docker build -t peer-ipx benchmarks/peers/ipx
python3 scripts/benchmark_peers.py --peer ipx --image peer-ipx \
  --fixtures corpus --pixels pixels --lane 0 --output peer-ipx-0.jsonl
```

Chaque fichier JSONL contient l’identité de l’image, de l’hôte et du corpus, suivie du résultat de chaque recette du groupe. Pour reconstituer le CSV des deux architectures, fournir les dix fichiers de chaque paire serveur/architecture à `scripts/benchmark_peer_report.py --csv comparaison.csv`. Ce script contrôle la couverture des 10 000 entrées et recalcule les métriques depuis les échantillons.
