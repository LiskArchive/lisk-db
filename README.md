# @liskhq/lisk-db

[![Build status](https://github.com/LiskHQ/lisk-db/actions/workflows/pr.yaml/badge.svg)](https://github.com/LiskHQ/lisk-db/actions/workflows/pr.yaml)
![npm](https://img.shields.io/npm/v/@liskhq/lisk-db)
![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/liskHQ/lisk-db)
![GitHub repo size](https://img.shields.io/github/repo-size/liskhq/lisk-db)
[![DeepScan grade](https://deepscan.io/api/teams/6759/projects/25497/branches/799572/badge/grade.svg)](https://deepscan.io/dashboard#view=project&tid=6759&pid=25497&bid=799572)
![GitHub issues](https://img.shields.io/github/issues-raw/liskhq/lisk-db)
![GitHub closed issues](https://img.shields.io/github/issues-closed-raw/liskhq/lisk-db)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](http://www.apache.org/licenses/LICENSE-2.0)

@liskhq/lisk-db is a database access implementation according to the Lisk protocol

## Installation

```sh
$ npm install --save @liskhq/lisk-db
```

## Get Started
```js
const { Database } = require('@liskhq/lisk-db');
const db = new Database('./new-database');

await db.set(Buffer.from([0]), Buffer.from('new item'));
const value = await db.get(Buffer.from([0]));
console.log(value);
```

## Dependencies
The following dependencies need to be installed in order to build this repository.

* [Node.js v18](https://nodejs.org)
* [Rust](https://www.rust-lang.org/)

## License

Copyright 2016-2023 Lisk Foundation

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
