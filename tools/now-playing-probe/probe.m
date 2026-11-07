// 시스템 지금 재생 중 정보 읽기(진단용) — probe.sh 가 빌드해 /usr/bin/perl 에 실어 실행한다.
#import <Foundation/Foundation.h>
#import <dlfcn.h>
typedef void (*GetInfo)(dispatch_queue_t, void(^)(CFDictionaryRef));
typedef void (*GetPlaying)(dispatch_queue_t, void(^)(Boolean));
typedef void (*GetClient)(dispatch_queue_t, void(^)(id));
__attribute__((constructor)) static void run(void) {
  setvbuf(stdout, NULL, _IONBF, 0);
  void *h = dlopen("/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote", RTLD_NOW);
  GetInfo gi = dlsym(h, "MRMediaRemoteGetNowPlayingInfo");
  GetPlaying gp = dlsym(h, "MRMediaRemoteGetNowPlayingApplicationIsPlaying");
  GetClient gc = dlsym(h, "MRMediaRemoteGetNowPlayingClient");
  dispatch_semaphore_t s = dispatch_semaphore_create(0);
  dispatch_queue_t q = dispatch_get_global_queue(0,0);
  gi(q, ^(CFDictionaryRef d){
    NSMutableDictionary *m = [NSMutableDictionary dictionary];
    for (id k in (__bridge NSDictionary*)d) { id v = ((__bridge NSDictionary*)d)[k];
      if ([v isKindOfClass:[NSData class]]) v = [NSString stringWithFormat:@"<data %lu bytes>", (unsigned long)[v length]];
      m[k] = [v description]; }
    printf("INFO %s\n", [[m description] UTF8String]); dispatch_semaphore_signal(s); });
  dispatch_semaphore_wait(s, DISPATCH_TIME_FOREVER);
  gp(q, ^(Boolean p){ printf("PLAYING %d\n", p); dispatch_semaphore_signal(s); });
  dispatch_semaphore_wait(s, DISPATCH_TIME_FOREVER);
  id (*lo)(void) = dlsym(h, "MRMediaRemoteGetLocalOrigin");
  void (*gso)(id, dispatch_queue_t, void(^)(CFArrayRef)) = dlsym(h, "MRMediaRemoteGetSupportedCommandsForOrigin");
  int (*gcmd)(id) = dlsym(h, "MRMediaRemoteCommandInfoGetCommand");
  Boolean (*gen)(id) = dlsym(h, "MRMediaRemoteCommandInfoGetEnabled");
  if (lo && gso && gcmd) { gso(lo(), q, ^(CFArrayRef arr){ printf("COMMANDS");
      for (id c in (__bridge NSArray*)arr) printf(" %d%s", gcmd(c), (gen && !gen(c)) ? "(off)" : "");
      printf("\n"); dispatch_semaphore_signal(s); });
    dispatch_semaphore_wait(s, dispatch_time(DISPATCH_TIME_NOW, 3*NSEC_PER_SEC)); }
  if (gc) { gc(q, ^(id c){ printf("CLIENT %s\n", [[c description] UTF8String]); dispatch_semaphore_signal(s); });
  dispatch_semaphore_wait(s, DISPATCH_TIME_FOREVER); }
}
