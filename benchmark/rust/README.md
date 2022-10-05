# Benchmarking Rust implementation of lisk-db library

## Prerequisites

To be able to successfully run benchmarks, following applications need to be installed on a system:

* [Valgrind](https://valgrind.org)

### macOS
To install *Valgrind* please follow instructions [here](https://github.com/LouisBrunner/valgrind-macos).

**Note:** *Valgrind* is not supported by Apple new CPUs M1 or M2. 

### Ubuntu
```sh
sudo apt-get install valgrind
```

* [KCachegrind](https://kcachegrind.sourceforge.net/html/Home.html)

### macOS
*KCachegrind* is not supported by macOS but its alternative exists. Application name is *QCacheGrind*.
```sh
brew install qcachegrind
```

### Ubuntu
```sh
sudo apt-get install kcachegrind
```

## **Note**
At the moment only Rust version **1.59** is supported. This is due to an [incompatibility issue](https://bugs.kde.org/show_bug.cgi?id=452758) between newer versions of Rust and *Valgrind* tool. When an issue will be resolved, this restriction is going to be removed.

## List of all benchmark applications

- bench_smt (Sparse Merkle Tree benchmarking)

## Running benchmark

```sh
$ ./run_benchmark.sh benchmark_name
```

Example:

```sh
$ ./run_benchmark.sh bench_smt
```

If a script successfully finishes, KCachegrind will be opened with a benchmarking result.

## License

Copyright 2016-2022 Lisk Foundation

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

[lisk core github]: https://github.com/LiskHQ/lisk
[lisk documentation site]: https://lisk.com/documentation/lisk-sdk/references/lisk-elements/db.html
