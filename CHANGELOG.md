# Changelog

## [0.5.0](https://github.com/danielcristho/croft/compare/v0.4.0...v0.5.0) (2026-11-24)


### Features

* **collector:** accept queue per listening socket on /metrics ([#54](https://github.com/danielcristho/croft/issues/54)) ([dc561a6](https://github.com/danielcristho/croft/commit/dc561a6a95d36efa84ef21d63212f83cae524374))


### Bug Fixes

* **labs:** calibrate per answered request and stop at a saturated level ([5bebd04](https://github.com/danielcristho/croft/commit/5bebd04fd5bfbf5f820c257240f5ff686e0c5c64))
* **labs:** recalibrate hotel-reservation at 6.18 ms per request, scale 1 ([5bebd04](https://github.com/danielcristho/croft/commit/5bebd04fd5bfbf5f820c257240f5ff686e0c5c64))
* **labs:** record pods a scale-down deleted, so first_ready_s is set ([5bebd04](https://github.com/danielcristho/croft/commit/5bebd04fd5bfbf5f820c257240f5ff686e0c5c64))
* **labs:** record the containers of pods a scale-down deleted mid-run ([#67](https://github.com/danielcristho/croft/issues/67)) ([758638e](https://github.com/danielcristho/croft/commit/758638e110cfd49f48e4cac4a7898fa49775cd8e))
* **labs:** stop calibration at the CPU limit, recalibrate otel-demo ([#70](https://github.com/danielcristho/croft/issues/70)) ([ea7fbb5](https://github.com/danielcristho/croft/commit/ea7fbb578582e4a74fe4eb407f11133be6214a54))

## [0.4.0](https://github.com/danielcristho/croft/compare/v0.3.2...v0.4.0) (2026-11-21)


### Features

* **collector:** in-flight requests per container from accepted connections ([#48](https://github.com/danielcristho/croft/issues/48)) ([7f1b78c](https://github.com/danielcristho/croft/commit/7f1b78cdf2ada9f5c55cba5bf720aa70f2c2f33d))


### Bug Fixes

* **ci:** drop trailing whitespace in release.yml ([#58](https://github.com/danielcristho/croft/issues/58)) ([0e899fe](https://github.com/danielcristho/croft/commit/0e899fec12191bde3276c608912eb1ec2ff644cd))

## [0.3.2](https://github.com/danielcristho/croft/compare/v0.3.1...v0.3.2) (2026-11-18)


### Bug Fixes

* kubectl stderr, one HTTP deadline, label unescape ([#45](https://github.com/danielcristho/croft/issues/45)) ([3a1ed00](https://github.com/danielcristho/croft/commit/3a1ed00845a366cbd52d91331ef174ccbabf29c2))

## [0.3.1](https://github.com/danielcristho/croft/compare/v0.3.0...v0.3.1) (2026-11-18)


### Bug Fixes

* **labs:** leave containers without an id out of the signal join ([#42](https://github.com/danielcristho/croft/issues/42)) ([f8d7079](https://github.com/danielcristho/croft/commit/f8d7079ffc4a646d6b9058bcf9c0f96ce2175210))

## [0.3.0](https://github.com/danielcristho/croft/compare/v0.2.0...v0.3.0) (2026-11-18)


### Features

* **labs:** load map of the services that take a burst (#110) ([15709ea](https://github.com/danielcristho/croft/commit/15709ea0298c8211fb34c26c9f68c73d78b44d5c))
* **labs:** record every service per run with its role and language (#108) ([00ae4e6](https://github.com/danielcristho/croft/commit/00ae4e62dbba40419179c4f802a2cb2627e8e312))
* **labs:** weighted endpoint mix and load config in every run record (#109) ([9b05769](https://github.com/danielcristho/croft/commit/9b05769615226417cf9faeea3e34a467c9d76a8d))
* **research:** croft python package for prometheus and dataset signals (#113) ([2691f10](https://github.com/danielcristho/croft/commit/2691f10808dc0f6fba85c64ed353091f87d7119d))


### Bug Fixes

* apache-2.0 license metadata, license and dco checks in ci (#114) ([2f200f2](https://github.com/danielcristho/croft/commit/2f200f2b37701e13fa580e12a69f75cb18bc8288))

## 0.2.0 (2026-11-12)


### Features

* add banner ([678a774](https://github.com/danielcristho/croft/commit/678a77488b67cdf02cbc817dfa2c32b8d5d15af0))
* add banner ([d442975](https://github.com/danielcristho/croft/commit/d442975369cdaea6122edf622b5e7229e6771c69))
* add banner, make the project modular ([f70b40e](https://github.com/danielcristho/croft/commit/f70b40e04c3eb0117430cb42942cbebcdf4c9366))
* add banner, make the project more modular, add syscall table ([3723530](https://github.com/danielcristho/croft/commit/3723530fabf6279286650937621fe13b432055d5))
* add banner, make the project more modular, add syscall table, ([c7e8ae2](https://github.com/danielcristho/croft/commit/c7e8ae23a8e527f3e1f5da90cf86a8902484dfed))
* add croft deploy through k8s ([7f1b3fa](https://github.com/danielcristho/croft/commit/7f1b3faa9293ad5ba684c49b5a14b18002abf428))
* add remaining syscall names, add deploy runbook ([ce1478a](https://github.com/danielcristho/croft/commit/ce1478a3807b8da6aee9b33cf1ba67acd6af651d))
* add remaining syscall names, add deploy runbook, add backfilling ([b3fb3b8](https://github.com/danielcristho/croft/commit/b3fb3b86ceebe9820c0f3f605e0e318737826f49))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry ([c1a3ea4](https://github.com/danielcristho/croft/commit/c1a3ea429d81cdcc586df9af27981e4038d83c8b))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry, add dataset, create json format ([c4cdf9a](https://github.com/danielcristho/croft/commit/c4cdf9a56286755e6bab78a46771c1655b6c51f9))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry, add dataset, create json format ([f3079cb](https://github.com/danielcristho/croft/commit/f3079cbc00e5669c3694f7d08a96ad2637c973df))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry, add dataset, create json format, add systemd ([cdf7e42](https://github.com/danielcristho/croft/commit/cdf7e4248c6e4639018c814974d0266561acf26e))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry, add dataset, create json format, add systemd ([1990673](https://github.com/danielcristho/croft/commit/19906739880e48e1e33e3807468b3ed862b4b555))
* add remaining syscall names, add deploy runbook, add backfilling, add new entry, add dataset, create json format, add systemd ([cf8f749](https://github.com/danielcristho/croft/commit/cf8f7497f5474321d7e1407717341b3a0bf159ff))
* add research progress ([6e3c871](https://github.com/danielcristho/croft/commit/6e3c871e7984994cb34d7834aade0f4de5fc372e))
* add some descriptions line ([eb22b8f](https://github.com/danielcristho/croft/commit/eb22b8f2757d3968b6c923b76958ac4094753d8a))
* **apps:** add online-boutique for project experiments ([e850490](https://github.com/danielcristho/croft/commit/e8504904c04caba3c423859cbcbf17818bd9ea16))
* **cluster:** collect each agent's recent segments into a node directory ([109f3fe](https://github.com/danielcristho/croft/commit/109f3fea083fc83ac4d0a21312092d26a74fc3ca))
* **cluster:** collect only the last 24 hours of each agent's dataset ([9018bee](https://github.com/danielcristho/croft/commit/9018bee5bf73c8b57dde366c06e78bbefbb2fb95))
* **collector:** add croft cli with run, top, stats, deploy and collect ([8c7b647](https://github.com/danielcristho/croft/commit/8c7b647c4429948f2a81e9706af7801a3f539027))
* **collector:** add namespace ([54493de](https://github.com/danielcristho/croft/commit/54493de2d9f33ce43e4887dabb53ed78f9d66044))
* **collector:** add new metrics ([5913bce](https://github.com/danielcristho/croft/commit/5913bcea4779bb28ff5994cfa8a8bf679f50c8ad))
* **collector:** count syscall bytes, connects and socket I/O (#52) ([ebdb573](https://github.com/danielcristho/croft/commit/ebdb57376a8d64c7a40974c88cb237c5ab405abd))
* **collector:** croft profile for per-container resource profiles ([d61a2e5](https://github.com/danielcristho/croft/commit/d61a2e56b69aa0d81181a54cd96cdb73cc45df69))
* **collector:** default dataset path /var/lib/croft for every command ([f8e93b7](https://github.com/danielcristho/croft/commit/f8e93b7141f222a27a832e58cf8f484181f9a1e5))
* **collector:** detect onnxruntime, xgboost and scikit-learn ([ff3735c](https://github.com/danielcristho/croft/commit/ff3735c1551610901e0acd13b9c8785f422f16ec))
* **collector:** detect pytorch, tensorflow and jax from mapped libraries ([0f6afed](https://github.com/danielcristho/croft/commit/0f6afed9c5a940477a6943f50caef385e9c31217))
* **collector:** detect serving, data and NLP frameworks, spark from its cmdline ([2b5dd8c](https://github.com/danielcristho/croft/commit/2b5dd8cce7209914c16311f1a9c4a2903992ecfa))
* **collector:** framework catalog in a json file with an override path ([ce734fd](https://github.com/danielcristho/croft/commit/ce734fdb6bd076d1f9d69d483b35f001ccfc1a13))
* **collector:** htop-style colours and in-place redraw for croft top ([c94447b](https://github.com/danielcristho/croft/commit/c94447b6ed81182a7b4e31b4681b2dcc02f68eb8))
* **collector:** idle heartbeat and skip empty gpu fields to cut dataset growth ([98f32cb](https://github.com/danielcristho/croft/commit/98f32cb5cba5f078abcdfa245e59792fed97a660))
* **collector:** metrics endpoint and croft scale ahead of the hpa ([5de37c0](https://github.com/danielcristho/croft/commit/5de37c0732597ad1ee822cd5cffaad55e31e8c4f))
* **collector:** metrics endpoint and croft scale ahead of the hpa ([a704a3d](https://github.com/danielcristho/croft/commit/a704a3d6e33e6d8b6c5a12be33fdb758111c0a66))
* **collector:** metrics endpoint and croft scale ahead of the hpa ([8c2b2b8](https://github.com/danielcristho/croft/commit/8c2b2b85014bd60ff6cdb0d6d1b27e10f1f201cf))
* **collector:** mmake more flexibility ([bf4f7a9](https://github.com/danielcristho/croft/commit/bf4f7a9facdbf50f9af681dbfe15a71aef0dd9e6))
* **collector:** modular refactor, record GPU holders with identity ([bfb4c63](https://github.com/danielcristho/croft/commit/bfb4c634d08f03721503dab812a8187364aadaa5))
* **collector:** record the working directory of each process ([1db5d1b](https://github.com/danielcristho/croft/commit/1db5d1b5dd9be58cfdd42bd6920caf9dcccb7fc4))
* **collector:** rotate the dataset into zstd segments with retention ([ba65254](https://github.com/danielcristho/croft/commit/ba652544a00b1f0ecadad7c559bb534daa584663))
* **collector:** run queue wait per process from sched tracepoints ([026b96d](https://github.com/danielcristho/croft/commit/026b96dd9655adccaa29fd3862eed42cac1f41c9))
* **collector:** scale down in croft scale, run it without the hpa (#49) ([84f4200](https://github.com/danielcristho/croft/commit/84f42002493457cf37c15ce098ab8cdddef56251))
* **collector:** wire croft cli into main, deserialize rows, rename binary ([68a2944](https://github.com/danielcristho/croft/commit/68a29448f6912d49b3ded368c68102edddd04827))
* **deploy:** publish the image to GHCR and add a Helm chart (#78) ([c6b09e2](https://github.com/danielcristho/croft/commit/c6b09e2fed28de9c6e8dea2a84021ac9f3c806da))
* **experiment:** calibrate Online Boutique, lift currencyservice's limit (#31) ([61d47c5](https://github.com/danielcristho/croft/commit/61d47c59ab69d98f45d48583b3581452eaa0fbde))
* **experiment:** merge compare scores, count pods, add Grafana dashboard (#26) ([0abcd49](https://github.com/danielcristho/croft/commit/0abcd4948b11751fc52b78b6c9a04eede76032ca))
* **experiment:** test Online Boutique, remove the webapp (#28) ([fd48a2c](https://github.com/danielcristho/croft/commit/fd48a2c8a39321a90ca02f96b088fba13d6a8e13))
* init collector using rust ([02760b6](https://github.com/danielcristho/croft/commit/02760b6a5fab1b9ba1f0efaffd2640f70d397f54))
* install from ghcr with helm, readme for the public release (#106) ([dadea74](https://github.com/danielcristho/croft/commit/dadea741e66bf5c31f11c7ea007127ab2e958497))
* **labs:** add ansible playbook to setup k3s cluster ([db67d82](https://github.com/danielcristho/croft/commit/db67d825aa50dfc22e9eb2a26cd6996ddc0853b5))
* **labs:** add research experiment harness for gpu and web workloads ([7de754c](https://github.com/danielcristho/croft/commit/7de754c64ab79813aeac25f7f6193b57d85dedea))
* **labs:** add research experiment harness for gpu and web workloads ([5d1a797](https://github.com/danielcristho/croft/commit/5d1a797576d1db39f6a1660f35d9cc0adf04b375))
* **labs:** adjust k3s config by adding argv ([533c30d](https://github.com/danielcristho/croft/commit/533c30d1d0864d0535acda4ee24088ce5aa53ad4))
* **labs:** adjust k3s config by adding argv ([e5720e6](https://github.com/danielcristho/croft/commit/e5720e638998811ad80b46ab25e4915869985b75))
* **labs:** apps.json and --scaler croft next to the hpa ([391b198](https://github.com/danielcristho/croft/commit/391b1987de0544d90701c5ab8102df920750ed87))
* **labs:** check webapp image on every node before web runs ([8d061dc](https://github.com/danielcristho/croft/commit/8d061dc64988b3a40e7e1b274caa8b654aa6e20e))
* **labs:** croft scale calibration for hotel-reservation ([2f0e5d0](https://github.com/danielcristho/croft/commit/2f0e5d0be5aaa7c6d715980ec80079a705f64609))
* **labs:** DeathStarBench hotelReservation as a harness app ([22a5f09](https://github.com/danielcristho/croft/commit/22a5f095788fa66a20d64e3d2e905e0e5d6f0a99))
* **labs:** export collector signals of every service per run (#75) ([fe910fd](https://github.com/danielcristho/croft/commit/fe910fd7f45c00518dedc51656a87d1c9b55c650))
* **labs:** just recipes and a guide for the sample applications ([0e3b416](https://github.com/danielcristho/croft/commit/0e3b416c4b10a82e99105676d42b41c41ee4e788))
* **labs:** just recipes for the service interaction study ([80b5c84](https://github.com/danielcristho/croft/commit/80b5c848595688567cc67da0f3e2b4849b116a4d))
* **labs:** KEDA arm, a ScaledObject on the ingress request rate ([fc7bf4e](https://github.com/danielcristho/croft/commit/fc7bf4e5dffddeb500edcbb68fc924fc3f59ffce))
* **labs:** linkerd call graph per run behind --mesh (#53) ([cf3cad3](https://github.com/danielcristho/croft/commit/cf3cad3373f705b269c32f65aba8589e5d6435e6))
* **labs:** log croft scale estimates every poll during runs ([0cffd6b](https://github.com/danielcristho/croft/commit/0cffd6b832f67729956e8022d972464d21e0ed81))
* **labs:** node speed calibration to node-speed.json ([e9ec778](https://github.com/danielcristho/croft/commit/e9ec778051f51695f602b1875747d1a03af3bda7))
* **labs:** one CPU training job per ML framework ([06734d0](https://github.com/danielcristho/croft/commit/06734d0a93f752e39326fd9bae14ade1fe198b77))
* **labs:** otel-demo calibration at its 300m limit ([ad1b246](https://github.com/danielcristho/croft/commit/ad1b2463f56f79f98519f4108cc443c214513c7c))
* **labs:** otel-demo calibration at its 300m limit ([2bb82e0](https://github.com/danielcristho/croft/commit/2bb82e0bad44e64b25ad56830c0d31647f127d5d))
* **labs:** otel-demo calibration, frontend at 300m and scale 1 ([8fed2f6](https://github.com/danielcristho/croft/commit/8fed2f6d46698cf3b6b4969cff5906282b3c0830))
* **labs:** otel-demo sample and a default scale per app ([8ade35b](https://github.com/danielcristho/croft/commit/8ade35b7339e3e29ba2637bcb7d5eb8e7248bcc2))
* **labs:** otel-demo sample, just recipes and a samples guide ([ce49a59](https://github.com/danielcristho/croft/commit/ce49a59288a31ab088d9d8063cd6049a8202fbeb))
* **labs:** ramp load shape and image preflight for web runs ([8e94e97](https://github.com/danielcristho/croft/commit/8e94e972eed592599ab681dd3ed268b98277a6e1))
* **labs:** reach the cluster over Tailscale ([168405e](https://github.com/danielcristho/croft/commit/168405e2f7719b542bf1e191c14d3dd821ddfaaa))
* **labs:** remove the keda arm, compare the hpa with croft (#48) ([423072f](https://github.com/danielcristho/croft/commit/423072f6f000ef508e347453b455a2f683225950))
* **labs:** run the interaction study's apps concurrently with --concurrent ([8e74af2](https://github.com/danielcristho/croft/commit/8e74af224cfdff780c13a1d17d5601f055f67dc7))
* **labs:** run.py calibrate-scaler measures an app's croft calibration ([5f407c5](https://github.com/danielcristho/croft/commit/5f407c596656c30130b8ea6425c8b1273b160d45))
* **labs:** run.py compare scores hpa and croft burst runs per arm ([5163853](https://github.com/danielcristho/croft/commit/51638531fdf24c376b2a81f0eff4a208eee79989))
* update bpf-linker ([8a33589](https://github.com/danielcristho/croft/commit/8a33589de77cbcc2a1337f7e70f838ad81ea07af))
* update bpf-linker, update docs, update just recipe ([c0c9fbb](https://github.com/danielcristho/croft/commit/c0c9fbb73b7257add80a73389ff4a25fd33c391d))
* update bpf-linker, update docs, update just recipe, update tracepoint ([a7fd295](https://github.com/danielcristho/croft/commit/a7fd2954a356327096c7f01681e7b656afb7f8a6))


### Bug Fixes

* ansible setup, add template prompts, add some adjusment ([019027b](https://github.com/danielcristho/croft/commit/019027b31596a2f34d6d5149699874ecd97cc10e))
* ansible setup, add template prompts, add some adjusment ([781eba6](https://github.com/danielcristho/croft/commit/781eba6f5ad15e29814b46be40d47e7598c8893f))
* ansible setup, add template prompts, add some adjusment ([04ce004](https://github.com/danielcristho/croft/commit/04ce00414f09ac45e2f114933dba55d9fb3c64e3))
* **collector:** exit on SIGINT and SIGTERM as container PID 1 ([7d90e39](https://github.com/danielcristho/croft/commit/7d90e3901e725d51280d80f1d4f790667f1da072))
* **collector:** log a repeated dry-run decision once (#88) ([651e240](https://github.com/danielcristho/croft/commit/651e240790640509e444d01fedb62db3c9988c5b))
* **collector:** log at info by default, RUST_LOG still overrides (#86) ([2324273](https://github.com/danielcristho/croft/commit/23242732572521c79573f7ee76e443e36455b730))
* **collector:** mark processes holding a GPU before attach as gpu-held ([dca19a0](https://github.com/danielcristho/croft/commit/dca19a061e35c43bc6733845b78eae93d25d3eee))
* **collector:** remove stray paste in top.rs ([1ab2466](https://github.com/danielcristho/croft/commit/1ab24664b2dba15b8bf7cb05522f2fd55651f16e))
* **collector:** restore the --help text of every command and option ([2f34ca6](https://github.com/danielcristho/croft/commit/2f34ca614dbebcff4593c31c6f4e1eeb9bdf08f9))
* **experiment:** count replicas from scaling decisions (#35) ([c531b49](https://github.com/danielcristho/croft/commit/c531b49d41c780dcc25edf01ff997d37a744cd93))
* **experiment:** keep Online Boutique's backends at one replica (#29) ([f3797c7](https://github.com/danielcristho/croft/commit/f3797c74c7172b3247ab512a9ad7079bc6c48e35))
* **experiment:** stop rejecting small images on full node image lists (#30) ([3ddf4e1](https://github.com/danielcristho/croft/commit/3ddf4e165b781680524ee43108fa17def555e0fb))
* **labs:** framework jobs finish without errors ([8e6fd50](https://github.com/danielcristho/croft/commit/8e6fd5060b4b294db5758036e380c454c084d6b2))
* **labs:** fresh pod per run and refuse finished pods that freeze the hpa ([c05c8f3](https://github.com/danielcristho/croft/commit/c05c8f3eb2c4cb665df4ce51558229989461445d))
* **labs:** generator timing, series names and setup notes (#70) ([1b2bc66](https://github.com/danielcristho/croft/commit/1b2bc66bd9465dc52fc9ddf67d985dd3481d147f))
* **labs:** just restart brings registries and stores up before the services ([a6b8312](https://github.com/danielcristho/croft/commit/a6b83121bd2c49ffedb1de29b4dcc434af47be82))
* **labs:** KEDA query filters on exported_namespace ([4436b6f](https://github.com/danielcristho/croft/commit/4436b6feab122f70d9b8b9554f93a5d71f63ee6c))
* **labs:** keep scaler.log when a burst run fails ([cf07233](https://github.com/danielcristho/croft/commit/cf0723303e715facbecf72388604dce5db3dbed3))
* **labs:** kubelet eviction reclaim 2Gi ([21eeaa0](https://github.com/danielcristho/croft/commit/21eeaa0c49d63099ba1e6f6a024f712a1bbd35ab))
* **labs:** kubelet eviction reclaim 2Gi ([db82be1](https://github.com/danielcristho/croft/commit/db82be182dfc4036462045a9678b61ff8da92bcb))
* **labs:** label linkerd proxy metrics with their deployment (#68) ([32cd004](https://github.com/danielcristho/croft/commit/32cd0048986eed041bc06078a62437e54ae00def))
* **labs:** otel frontend at 512Mi, and one burst directory per arm ([e8e41f0](https://github.com/danielcristho/croft/commit/e8e41f0b0f184068d295c49d36f35de464d56120))
* **labs:** otel-demo postgres memory and failed requests in the summary (#69) ([d75f546](https://github.com/danielcristho/croft/commit/d75f5465e245422378ba8d2dcebb473158de2549))
* **labs:** raise otel-demo product-catalog and db CPU limit to 2 (#81) ([a928d25](https://github.com/danielcristho/croft/commit/a928d2535a29c00a0f8d592867c42af212d054ee))
* **labs:** raise otel-demo product-catalog memory to 256Mi (#80) ([3dfef80](https://github.com/danielcristho/croft/commit/3dfef806dc536d9cf3a3b4c6ca75502d6fdfdfe4))
* **labs:** read croft scale-downs in the replica steps (#50) ([a5e9ad0](https://github.com/danielcristho/croft/commit/a5e9ad0d3fa00e7970f300f9a1c45a63d5c78838))
* **labs:** read croft scale-ups from its coloured log ([6a9534b](https://github.com/danielcristho/croft/commit/6a9534ba41bf67a0570546cd1793ed60dc84d3ff))
* **labs:** record CPU throttling as the throttled share of CFS periods ([0f77ea8](https://github.com/danielcristho/croft/commit/0f77ea8c7437feb26f90b6645e633a24eb98033d))
* **labs:** restart order, otel frontend memory, one burst directory per arm ([7bf1211](https://github.com/danielcristho/croft/commit/7bf1211d1b8a0e8920c8969227c70d0b387d0a06))
* **top:** scroll under a fixed header and quit with q (#76) ([961ca67](https://github.com/danielcristho/croft/commit/961ca67efad574fa0a0c540c0be26629c723b734))
* **top:** scroll under a fixed header and quit with q (#77) ([f5ddb59](https://github.com/danielcristho/croft/commit/f5ddb5997ef45db75a211463a60de1c905c5fe0e))
* **website:** deploy from Cloudflare instead of GitHub Pages (#72) ([f5c1157](https://github.com/danielcristho/croft/commit/f5c1157d6319574f7c315796b991f1a5095969c1))
