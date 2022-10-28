use std::error::Error;
use std::sync::mpsc;

use rocksdb::{Options, DB};
use tempdir::TempDir;

use lisk_db::batch::PrefixWriteBatch;
use lisk_db::consts;
use lisk_db::db::types::{DbMessage, Kind};
use lisk_db::db::DB as LDB;
use lisk_db::smt::{SparseMerkleTree, UpdateData};
use lisk_db::smt_db;
use lisk_db::types::{Cache, KeyLength, NestedVec, SharedKVPair};

const KEYS: [&str; 90] = [
    "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
    "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
    "265fda17a34611b1533d8a281ff680dc5791b0ce0a11c25b35e11c8e75685509",
    "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
    "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
    "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
    "9652595f37edd08c51dfa26567e6cd76e6fa2709c3e578478ca398d316837a7a",
    "cdb4ee2aea69cc6a83331bbe96dc2caa9a299d21329efb0336fc02a82e1839a8",
    "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
    "3973e022e93220f9212c18d0d0c543ae7c309e46640da93a4a0314de999f5112",
    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
    "8a5edab282632443219e051e4ade2d1d5bbc671c781051bf1437897cbdfea0f1",
    "684888c0ebb17f374298b65ee2807526c066094c701bcc7ebbe1c1095f494fc1",
    "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
    "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
    "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
    "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
    "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
    "951dcee3a7a4f3aac67ec76a2ce4469cc76df650f134bf2572bf60a65c982338",
    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068",
    "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
    "32ebb1abcc1c601ceb9c4e3c4faba0caa5b85bb98c4f1e6612c40faa528a91c9",
    "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
    "bb7208bc9b5d7c04f1236a82a0093a5e33f40423d5ba8d4266f7092c3ba43b62",
    "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
    "334359b90efed75da5f0ada1d5e6b256f4a6bd0aee7eb39c0f90182a021ffc8b",
    "09fc96082d34c2dfc1295d92073b5ea1dc8ef8da95f14dfded011ffb96d3e54b",
    "8a331fdde7032f33a71e1b2e257d80166e348e00fcb17914f48bdb57a1c63007",
    "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
    "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
    "ba5ec51d07a4ac0e951608704431d59a02b21a4e951acc10505a8dc407c501ee",
    "ffe679bb831c95b67dc17819c63c5090d221aac6f4c7bf530f594ab43d21fa1e",
    "d03502c43d74a30b936740a9517dc4ea2b2ad7168caa0a774cefe793ce0b33e7",
    "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
    "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
    "a318c24216defe206feeb73ef5be00033fa9c4a74d0b967f6532a26ca5906d3b",
    "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
    "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
    "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
    "1f18d650d205d71d934c3646ff5fac1c096ba52eba4cf758b865364f4167d3cd",
    "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
    "bbf3f11cb5b43e700273a78d12de55e4a7eab741ed2abf13787a4d2dc832b8ec",
    "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
    "e65e0438387c009ed1e9f698ef950883c17630189f0a3c49044211bff1540187",
    "bcd58d5a7545e4cca782e09db6d78e48d32576bcfff2810472902cbeb6bb4ab4",
    "51ffd19f7dcae1a36a744525c69e3003071f10add81fcaa15f2bc5b42a1b8a9e",
    "11c902d402a6036db35adb25a0686c067d760350c883dc83d9911d85318b9f69",
    "c1c11e91be3ed32e1cec1883e6a1a5276b3cdcb96486aa1a182f7b2eb0202e40",
    "8469b977b07ccab45736dfce7b988cdc97f8452678dd5b7766bf83ed014068b8",
    "0f146dcdf05d1b016d51e00dea1cc86b823538994517e488a395023c6a66f1de",
    "c417b45b8c8f0eba8912e5127cf5f52db20b0bb9d4d92d1e7d2efd50541592d2",
    "7d9ea2b8eed8093b367141fa8dac0f9e0e9e72feb3cb3cb2d71443a8d660cc17",
    "ff7edcbd26282ea61433cdb4e518d722400dcdd8240400b7b4b656bd4e2fef56",
    "2dd2646dcb0bad61779e1565c24edda473270e8be978630ab80d202333f5fe23",
    "0bfe903876794d3a380f801cadb81ea0b8d7e62de9b9b345ad539a784ddaec08",
    "c5aa9c7a69f0068fd2f33aa400741a642dc019113a31ce80d878837059497422",
    "03a286d8d276718c59380049124542ac3f26752b76f8837f81e9c9285d724b53",
    "1ed9d072e584d448cf7f1c8fa6372fad2cba7826beb26ecad96d951afc8c97aa",
    "7e5ecfc310e95af0ff717ec2c172bd437a6b953bc600bdb48c8bb63d822b7bde",
    "45076c98300ebcbbe42ba686bac81c8e00f2b3295e768671ae2e78ffcc93dbd4",
    "e74b0d9a2e39bbdef4ecf59381e28a1402f41a51c422074f1551bd0ec5b36a40",
    "72cdbe90bf16be3025dfac5ada60a20fd2f989d9d27a03644733198a383d39cc",
    "25bda02c8cfacd6471d8db437aaff0e503e278f5d4c7f22b2b5ad8b6c386ff8e",
    "daa45800330f1534835d0bb1071f620504ccff3091148eae5f384c78493bf1b9",
    "5ff131029fa7d03ee0ebb50c60aac0f6ae519f2a85f0445350e31e2e6ea0c57c",
    "6e5f1d6aa1bc34031f2bb3b591ea21753c7a92b762323fca9e565b907ba49e16",
    "c8d3b95752ec61de3f63f931231ef0e6b4a6d49c0a4971f0ed9fe2ee5cf9bbde",
    "6f9319fa5596ee577c6e1c86fc0501918b6a54e435aa785d3e3084dfcd44286b",
    "9c42a002b34b1d99bce48c53b4dbfb4b0d9e907ce9f7ba9b593b7351ef9f1508",
    "f0ca64ee197198eefa3293cb78221b5c997764dadacef2e86a92068f14b82ddc",
    "030833e28118814afb86eafd910fbe8e998e2a04bb192bc6893c00b42ec6f3f0",
    "68476abe34baebead9d2503388966daba065026c9002b91c42dc1a47a78cbcee",
    "97191336c172c994da49d4380fd47cd66522903cdccb306b37558894febce162",
    "b207eff173823f3b3cb800f03abad517a13bf03d4e68dbd9d0235889fd788eb6",
    "4f65e0f25892502b2b855d31e59b40d2c3b60bd3b0ce4cdf5fb2bf7b14d0808b",
    "7f9d2945289ab2480fb67478fb326e47215307d50f2a0c3e2ff966ed3f1e7e50",
    "7a9f4b671a8bd60d9cb759fa83d2fca52f6a7211c358d6dc4819e6c131ea39cf",
    "8d07667fce612c4043a9d743dabf61f955df1bda833dad0783c7cc4dc24c1725",
    "05e1816a3886676bac63a268ba37e6438bcf8d9ca9b9209dd25b542f1ae83eb3",
    "dab3b322c9c463bc7876839a57d731dec216e8c04914bccd9032cb753dc94758",
    "1e38a217d1765129405a31b64f0572065be47b40556d80733cf26e3488fd1b70",
    "7ace3741886764a6c0cc4a18336b5d082872100dfee64cb802c3003fbec2ba6a",
    "65f3266907caabc300bd80905f84bb44bdaf8d27fbbb063fc6a5dff0370243eb",
    "da4778352339399216a80a0d30237126bb712341c9122752fd9657956cb540b0",
    "e631b62bf62181e39876f0049f53502a321ff0991866ce543af546dd2e0078f7",
];

const VALUES: [&str; 90] = [
    "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
    "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
    "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
    "e368b5f8bac32462da14cda3a2c944365cbf7f34a5db8aa4e9d85abc21cf8f8a",
    "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
    "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
    "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
    "2651c51550722c13909ec43f50a1da637f907f7a307e8f60695ae23d3380abad",
    "8a1fe157beac6df9db1f519afed60928eeb623c104a53a62af6b88c423d47e35",
    "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
    "fc62b10ec59efa8041f5a6c924d7c91572c1bbda280d9e01312b660804df1d47",
    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
    "e344fcf046503fd53bb404197dac9c86405f8bd53b751e40bfa0e386df112f0f",
    "ff122c0ea37f12c5c0f330b2616791df8cb8cc8f1114304afbf0cff5d79cec54",
    "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
    "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
    "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
    "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
    "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
    "bf1d535d30ebf4b7e639721faa475ea6e5a884f6468929101347e665b90fccdd",
    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
    "24944f33566d9ed9c410ae72f89454ac6f0cfee446590c01751f094e185e8978",
    "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
    "d8909983be3179a28734edac2ad9e1d0364c8e15e6e8cdc1363d9969d23c7d95",
    "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
    "7e0e5207f9102c79bd355ccafc329b517e0d6c2b509b37f30cfc39538992cb36",
    "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
    "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
    "7c8b1ed7e1074189bf7ff3618e97b4ce88f25c289f827c30fabfbe6607058af6",
    "d4880b6be079f51ee991b52f2e92636197de9c8a4063f69987eff619bb934872",
    "ef79a95edac9b7119192b7765ef48dc8c7ecc0db12b28d9f39b5b4dedcc98ccd",
    "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
    "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
    "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
    "2ec9b3cc687e93871f755d6c9962f62f351598ba779d9838aa2b68c8ef309f50",
    "084aa2e4bc2defcd2c409c1916023acd6972624cf88112280b6dacde48367b0c",
    "0ca5765ffb7eb99901483c2cda1dd0209cef517e96e962b8c92c1668e5334d43",
    "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
    "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
    "0272614c70432bc1f94b8739603ba170ad5f4866a9936f37c463767bda7d005d",
    "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
    "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
    "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
    "a2215262d04d393d4e050d216733138a4e28c0b46375e84b7a34a218db8dc856",
    "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
    "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
    "247f88a674f9f504e95f846272b120deaa29b0ae6b0b9069689488ba3c8e90ab",
    "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
    "be3cc7d4abe4c8a28ca784da0313866493715dc24cf94db3612b279b14b82bc5",
    "92269bc9c3317711febf6d0fc0e84a1c1c2ff7af86980862df38657fe93a6c72",
    "ebb9352836c78f502f4414c34484b2960db46a214b14b66a45b0f81b7aadf651",
    "87d47cd68518a5eef838fc3d9c8291ef05e54902f2a229d8b4b5e3c2fdd863d4",
    "d8d787c71350f6eb8ebff10dba454099e6b06af5a4a2af24dff06493aef35e28",
    "5dc694c88d3578bdf79b39ba4971a92835bfbfcc64bd46c37bd22bc199bf2720",
    "47dc94eb404454f48d25336448afeef0593fe0d28da42e4554597a69a6f35199",
    "0d68b9bb2d214605cf759d4c659df45f4491e57566121499672f40a913f20690",
    "821634f5c95d5f24b06ba36adfd3ad98db4239e4b43f8671f14b6b023a6cf794",
    "c65a5d87fa4fe2cbfb4c5c0037fe26fc3811f9a0051aa41394d19122eccd3fb1",
    "78131360107b7ab455ded402c62053bb0cb0199e9fd797917ae6da507e4e8738",
    "c5941f2a43b02014e376f4f18c9945468badcc7b34f590a8da781708699f3cd6",
    "29ab7601bf6cc8d8704e3628e048827bb0fdd28037c039c4742f5b51bb7819fb",
    "02a46b805b5c4341ad4742fb2c612481ae09f1ff171e8f994083816aa1656686",
    "62d4a4a8a92682406e168bfbc0584634698c43aa25211336042cb3e3bdda2f71",
    "5eff7d54248cce2d8e07607db394f3e12e855dab68cb5c4b8fbf6d2839bb2db6",
    "3cf9c842351fbf8e5fe1414dc796bff5fcda99f3bc07a068721829ce2077634f",
    "cad73cfc73bd7c68e6135f5635b560ab8aa29be4d65edd2ceeed9a671634b44a",
    "3189fda2a5bfffa63ac02b7bba960c773716b10b07927d4886eb397860044753",
    "6804478e219c91ad556da7eaa36119c46126c90fd5fc7a220aff8a077173f02c",
    "b8d08f1e5313a049fa5d121672a1569d0385ebd515083af866e99542658866aa",
    "7f0edc694ee0b9c8c265011958b5edf63f6ccf9f563c5a417599000b9ffdcdae",
    "ef542593562b8016e0e1185d29716bfb29fe3814eb41ed687816f98c492ca169",
    "38a2a14740e60fe80113ef9bc92a302415ca049473a5e583f1a39ed462ba8b2a",
    "876b679aeba06f9f39adfb5d844df37cd96d44f43aaa064396a6226621e38dbd",
    "37cce57b1ae88947dc4bb425f11dfdecf125055d793268146c160ca063f271c6",
    "f87fca626fcf04a5145758f3aa2d7a1d3f3296bad11451c5c40b96776fb2af20",
    "73e7fb87bb9cfd02067078780de4c558e24e4239c37636bf3f77e587bd5b7d19",
    "caa00c4edb3afbfa556fdbe4097534b7c438117180e0cda07bf08020b8642367",
    "41ce0d96700efac2f55b2b4debee94c7d836135202ee799b87995a58915b136a",
    "91e6a214cc2ffbee7e856d6d88ef2536192811269b91392b2cb8a3e5fd7233ec",
    "033251172c794a46ace75bacbb4d3099c9babee89f1145b44b4c021eada78fe2",
    "c6c7efc334bd3464844ab8119dc874215d1e6b7d248a6a67e90188647a0399d9",
    "089b6bff49e8b211f521ac5935cf97dd671759981b103d904ce5ce8ab4ba6cdb",
    "b38055967a73f4432bf72df2bf4cfdeeb1c62ad16e242e313d58b840f365eb57",
    "c6f049bed5b0485fa00b9c6aaf68d977602be815671b1fc919cbe94d6ca3c2cc",
    "44fec2c45aaa328a2d9dd8c9c36ec6eae27c04ffc4695f2ec5a9ab8711b0f03a",
    "aa4beba1f8ef875a37581aa8b13d523234c8e3860184ea7edec9a47adbaf5d4b",
    "e2f173a93d0320aa69996b7909bec6dcdb7a42d132f321e02913ad97c934edd3",
    "92e3d2b54e9b2cba1f252000b56987733824619813f7945b9aebc629146f5fe8",
    "d7fd6c81cd252cef4d084300ac4810096ffd4ebfa2f63593921fc7f076c975d2",
    "a1f3e1a69066d16b04c826960420f19e155169a05ec2c67cc174bf13ae1c55bf",
];

const QUERY_KEYS: [&str; 10] = [
    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
    "ba5ec51d07a4ac0e951608704431d59a02b21a4e951acc10505a8dc407c501ee",
    "bcd58d5a7545e4cca782e09db6d78e48d32576bcfff2810472902cbeb6bb4ab4",
    "7e5ecfc310e95af0ff717ec2c172bd437a6b953bc600bdb48c8bb63d822b7bde",
    "7a9f4b671a8bd60d9cb759fa83d2fca52f6a7211c358d6dc4819e6c131ea39cf",
];

fn main() -> Result<(), Box<dyn Error>> {
    for _ in 0..10 {
        let mut data = UpdateData::new_from(Cache::new());
        for i in 0..KEYS.len() {
            data.insert(SharedKVPair(
                &hex::decode(KEYS[i])?,
                &hex::decode(VALUES[i])?,
            ));
        }

        let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());

        let temp_dir = TempDir::new("bench_db_")?;
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let rocks_db = DB::open(&opts, temp_dir.path())?;
        let (tx, _) = mpsc::channel::<DbMessage>();
        let common_db = LDB::new(rocks_db, tx, Kind::Normal);
        let mut db = smt_db::SmtDB::new(&common_db);

        let root = tree.commit(&mut db, &data)?;

        // write batch to RocksDB
        let mut write_batch = PrefixWriteBatch::new();
        write_batch.set_prefix(&consts::Prefix::SMT);
        db.batch.iterate(&mut write_batch);
        common_db.write(write_batch.batch)?;

        let proof = tree.prove(
            &mut db,
            &QUERY_KEYS
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>(),
        )?;

        let is_valid = SparseMerkleTree::verify(
            &QUERY_KEYS
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>(),
            &proof,
            &root.lock().unwrap(),
            KeyLength(32),
        )?;

        assert!(is_valid);
    }

    println!("Benchmarking successfully completed");

    Ok(())
}
