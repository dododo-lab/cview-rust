# heap 비교 — 2분 시점과 10분 시점(치지직 720p60)

`heap` 명령으로 찍었다. 찍는 동안 프로세스가 잠시 멈춰 그 시점의 측정값(앞당김 등)은 흐트러진다. 메모리 비교에만 쓴다.

## m1 보간 전체(복사 방식)

- footprint: 70.2M → 94.9M
- malloc 합계: 17.3MB → 28.7MB

| 늘어난 바이트 | 늘어난 개수 | 클래스 | 라이브러리 |
| ---: | ---: | --- | --- |
| 1,229,856 | 21,204 | `CFString` | CoreFoundation |
| 875,072 | 2,930 | `CFHTTPMessage` | CFNetwork |
| 500,224 | 1,954 | `NSURLRequestInternal` | CFNetwork |
| 437,696 | 977 | `NWConcrete_nw_data_transfer_report` | Network |
| 436,992 | 976 | `Swift.StringStorage` | libswiftCore.dylib |
| 384,784 | 2,654 | `NSMutableDictionary (Storage)` | CoreFoundation |
| 364,768 | 2,950 | `CFDictionary (Value Storage)` | CoreFoundation |
| 363,440 | 2,947 | `CFDictionary (Key Storage)` | CoreFoundation |
| 312,640 | 977 | `__CFN_TransactionMetrics` | CFNetwork |
| 250,112 | 977 | `FigMetricEvent` | CoreMedia |
| 188,800 | 2,950 | `CFDictionary` | CoreFoundation |
| 188,032 | 1,960 | `NSURL` | CoreFoundation |
| 158,240 | 989 | `NWConcrete_nw_activity` | Network |
| 150,528 | 168 | `std::__shared_ptr_emplace<Espresso::BNNSEngine::convolution_kernel>` | Espresso |
| 150,528 | 168 | `std::__shared_ptr_emplace<Espresso::ANERuntimeEngine::convolution_kern` | Espresso |

## m2 기준(보간·보호 끔, AVPlayer 단독)

- footprint: 39.2M → 48.9M
- malloc 합계: 15.5MB → 22.2MB

| 늘어난 바이트 | 늘어난 개수 | 클래스 | 라이브러리 |
| ---: | ---: | --- | --- |
| 1,058,128 | 16,200 | `CFString` | CoreFoundation |
| 878,048 | 2,941 | `CFHTTPMessage` | CFNetwork |
| 502,528 | 1,963 | `NSURLRequestInternal` | CFNetwork |
| 439,040 | 980 | `NWConcrete_nw_data_transfer_report` | Network |
| 375,232 | 978 | `Swift.StringStorage` | libswiftCore.dylib |
| 316,784 | 2,940 | `CFDictionary (Value Storage)` | CoreFoundation |
| 315,504 | 2,940 | `CFDictionary (Key Storage)` | CoreFoundation |
| 313,600 | 980 | `__CFN_TransactionMetrics` | CFNetwork |
| 250,624 | 979 | `FigMetricEvent` | CoreMedia |
| 188,416 | 1,963 | `NSURL` | CoreFoundation |
| 188,160 | 2,940 | `CFDictionary` | CoreFoundation |
| 158,560 | 991 | `NWConcrete_nw_activity` | Network |
| 109,760 | 980 | `__CFN_TaskMetrics` | CFNetwork |
| 109,648 | 979 | `NSURLResponseInternal` | CFNetwork |
| 94,016 | 2,938 | `HTTPHeaderDict` | CFNetwork |

