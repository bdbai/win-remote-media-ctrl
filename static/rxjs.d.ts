declare type Rxjs = typeof import('rxjs') & { webSocket: typeof import('rxjs/webSocket') };
declare const rxjs: Rxjs

type Observable<T> = import('rxjs').Observable<T>
type Subject<T> = import('rxjs').Subject<T>
type Subscriber<T> = import('rxjs').Subscriber<T>
type Subscription = import('rxjs').Subscription
