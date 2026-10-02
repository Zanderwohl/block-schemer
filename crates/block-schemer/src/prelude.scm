;; Where Steel differs from R7RS. A Steel original this redefines is kept as
;; `__steel-<name>` by `scheme.rs`, run before this.

;; R7RS's definition, as Steel's drops the value of a clause that is only a test.
(define-syntax cond
  (syntax-rules (else =>)
    [(cond [else e1 e2 ...]) (begin e1 e2 ...)]
    [(cond [test => receiver]) (let ([__cond-value test]) (when __cond-value (receiver __cond-value)))]
    [(cond [test => receiver] clause1 clause2 ...)
     (let ([__cond-value test]) (if __cond-value (receiver __cond-value) (cond clause1 clause2 ...)))]
    [(cond [test]) test]
    [(cond [test] clause1 clause2 ...)
     (let ([__cond-value test]) (if __cond-value __cond-value (cond clause1 clause2 ...)))]
    [(cond [test e1 e2 ...]) (when test e1 e2 ...)]
    [(cond [test e1 e2 ...] clause1 clause2 ...) (if test (begin e1 e2 ...) (cond clause1 clause2 ...))]))

;; Steel's own `=` takes exactly two arguments.
(define (= first . rest)
  (let loop ((a first) (rest rest))
    (or (null? rest)
        (and (__steel-= a (car rest)) (loop (car rest) (cdr rest))))))

(define (boolean=? a b . rest)
  (and (boolean? a) (boolean? b) (eq? a b)
       (or (null? rest) (apply boolean=? b rest))))

(define (gcd . ns)
  (let loop ((acc 0) (ns ns))
    (if (null? ns) acc (loop (__steel-gcd acc (car ns)) (cdr ns)))))

(define (lcm . ns)
  (let loop ((acc 1) (ns ns))
    (if (null? ns) acc (loop (__steel-lcm acc (car ns)) (cdr ns)))))

(define (atan y . x)
  (if (null? x)
      (__steel-atan y)
      (__atan2 (inexact y) (inexact (car x)))))

;; The simplest rational within `y` of `x`, worked exactly.
(define (rationalize x y)
  (define (simplest-positive lo hi)
    (let ((whole (floor lo)))
      (cond ((= whole lo) whole)
            ((< whole (floor hi)) (+ whole 1))
            (else (+ whole (/ 1 (simplest-positive (/ 1 (- hi whole)) (/ 1 (- lo whole)))))))))
  (define (simplest lo hi)
    (cond ((positive? lo) (simplest-positive lo hi))
          ((negative? hi) (- (simplest-positive (- hi) (- lo))))
          (else 0)))
  (let* ((ex (exact x)) (ey (abs (exact y)))
         (r (simplest (- ex ey) (+ ex ey))))
    (if (and (exact? x) (exact? y)) r (inexact r))))

(define (make-list k . fill)
  (let ((fill (if (null? fill) #f (car fill))))
    (let loop ((k k) (acc '()))
      (if (<= k 0) acc (loop (- k 1) (cons fill acc))))))

(define (list-copy obj)
  (let loop ((obj obj) (reversed '()))
    (if (pair? obj)
        (loop (cdr obj) (cons (car obj) reversed))
        (let rebuild ((reversed reversed) (tail obj))
          (if (null? reversed) tail (rebuild (cdr reversed) (cons (car reversed) tail)))))))

(define (member x list . compare)
  (if (null? compare)
      (__steel-member x list)
      (let loop ((list list))
        (cond ((null? list) #f)
              (((car compare) x (car list)) list)
              (else (loop (cdr list)))))))

(define (assoc x alist . compare)
  (if (null? compare)
      (__steel-assoc x alist)
      (let loop ((alist alist))
        (cond ((null? alist) #f)
              (((car compare) x (car (car alist))) (car alist))
              (else (loop (cdr alist)))))))

(define (string-copy string . range)
  (let ((start (if (null? range) 0 (car range)))
        (end (if (or (null? range) (null? (cdr range))) (string-length string) (car (cdr range)))))
    (substring string start end)))

;; Steel's takes no converter. Without `parameterize`, only the initial
;; value is ever converted.
(define (make-parameter value . converter)
  (__steel-make-parameter (if (null? converter) value ((car converter) value))))

(define (string-map proc string . strings)
  (list->string (apply map proc (string->list string) (map string->list strings))))

(define (string-for-each proc string . strings)
  (apply for-each proc (string->list string) (map string->list strings)))

(define (vector-map proc vector . vectors)
  (list->vector (apply map proc (vector->list vector) (map vector->list vectors))))

(define (vector-for-each proc vector . vectors)
  (apply for-each proc (vector->list vector) (map vector->list vectors)))

;; Handlers are kept here rather than in Steel, whose handler
;; replaces the value of the whole call and so cannot continue a raise.

(struct __error-object (message irritants))

(define __handlers '())

;; Set while an exception goes past every handler to end the run, so the
;; Steel handler each `with-exception-handler` installs lets it through.
(define __uncaught #f)

;; How many handlers a Steel error raised inside a handler is for: those
;; outside it. Set where it is raised, as unwinding restores `__handlers`
;; before any Steel handler runs. A count, as Steel's `cdr` is never `eq?`
;; to the list it came from.
(define __target #f)

(define (__escape message . irritants)
  (set! __uncaught #t)
  (apply __steel-error message irritants))

(define (__unhandled obj)
  (cond ((__error-object? obj)
         (apply __escape (__error-object-message obj) (__error-object-irritants obj)))
        ((__steel-error-object? obj)
         (set! __uncaught #t)
         (raise-error obj))
        (else (__escape "uncaught exception:" obj))))

(define (raise-continuable obj)
  (if (null? __handlers)
      (__unhandled obj)
      (let ((outer __handlers))
        (dynamic-wind
          (lambda () (set! __handlers (cdr outer)))
          (lambda ()
            (call-with-exception-handler
              (lambda (e)
                (if (not __target) (set! __target (length (cdr outer))))
                (raise-error e))
              (lambda () ((car outer) obj))))
          (lambda () (set! __handlers outer))))))

(define (raise obj)
  (raise-continuable obj)
  (__escape "a handler returned from raise:" obj))

;; Steel's own errors, such as `(car '())`, have already unwound when the
;; Steel handler sees them, so a handler that returns is an error, as after
;; `raise`.
(define (with-exception-handler handler thunk)
  (set! __uncaught #f)
  (set! __target #f)
  (let* ((outer __handlers) (mine (cons handler outer)))
    (dynamic-wind
      (lambda () (set! __handlers mine))
      (lambda ()
        (call-with-exception-handler
          (lambda (e)
            (if (or __uncaught (and __target (not (= __target (length mine)))))
                (raise-error e)
                (begin
                  (set! __target #f)
                  (set! __handlers outer)
                  (handler e)
                  (__escape "a handler returned from an error:" (error-object-message e)))))
          thunk))
      (lambda () (set! __handlers outer)))))

(define (error message . irritants)
  (raise (__error-object message irritants)))

(define (error-object? obj)
  (or (__error-object? obj) (__steel-error-object? obj)))

(define (error-object-message obj)
  (if (__error-object? obj) (__error-object-message obj) (__steel-error-object-message obj)))

(define (error-object-irritants obj)
  (if (__error-object? obj) (__error-object-irritants obj) '()))

;; With no file system, and Steel's reader errors indistinguishable from others.
(define (file-error? obj) #f)
(define (read-error? obj) #f)

;; Without `delay`, every promise is already forced.

(struct __promise (value))

(define (make-promise obj)
  (if (__promise? obj) obj (__promise obj)))

(define (promise? obj) (__promise? obj))

(define (force obj)
  (if (__promise? obj) (__promise-value obj) obj))

;; Every port but the console's is in memory, so it is always ready.

(define __closed '())
(define __binary '())

(define (close-port port)
  (set! __closed (cons port __closed))
  (__steel-close-port port))

(define (close-input-port port)
  (set! __closed (cons port __closed))
  (__steel-close-input-port port))

(define (close-output-port port)
  (set! __closed (cons port __closed))
  (__steel-close-output-port port))

(define (input-port-open? port)
  (and (input-port? port) (not (memq port __closed))))

(define (output-port-open? port)
  (and (output-port? port) (not (memq port __closed))))

(define (open-input-bytevector bytevector)
  (let ((port (__steel-open-input-bytevector bytevector)))
    (set! __binary (cons port __binary))
    port))

(define (open-output-bytevector)
  (let ((port (__steel-open-output-bytevector)))
    (set! __binary (cons port __binary))
    port))

(define (binary-port? obj)
  (and (port? obj) (memq obj __binary) #t))

(define (textual-port? obj)
  (and (port? obj) (not (memq obj __binary))))

(define (char-ready? . port)
  (not (eq? (if (null? port) (current-input-port) (car port)) __in)))
(define (u8-ready? . port) #t)

(define (read-string k . port)
  (let ((port (if (null? port) (current-input-port) (car port))))
    (let loop ((k k) (chars '()))
      (if (<= k 0)
          (list->string (reverse chars))
          (let ((c (read-char port)))
            (cond ((not (eof-object? c)) (loop (- k 1) (cons c chars)))
                  ((null? chars) c)
                  (else (list->string (reverse chars)))))))))

(define (read-bytevector! bytevector . args)
  (let* ((port (if (null? args) (current-input-port) (car args)))
         (range (if (null? args) '() (cdr args)))
         (start (if (null? range) 0 (car range)))
         (end (if (or (null? range) (null? (cdr range))) (bytevector-length bytevector) (car (cdr range)))))
    (let loop ((i start))
      (if (>= i end)
          (- i start)
          (let ((byte (read-u8 port)))
            (cond ((not (eof-object? byte)) (bytevector-u8-set! bytevector i byte) (loop (+ i 1)))
                  ((= i start) byte)
                  (else (- i start))))))))

(define (bytevector-copy! to at from . range)
  (let ((source (apply bytevector-copy from range)))
    (let loop ((i 0))
      (if (< i (bytevector-length source))
          (begin
            (bytevector-u8-set! to (+ at i) (bytevector-u8-ref source i))
            (loop (+ i 1)))))))

;; Steel's `(apply f x (cdr rest))` passes `(car rest)` for `x`; one consed
;; list avoids it.
(define (write-string string . args)
  (if (or (null? args) (null? (cdr args)))
      (apply __steel-write-string string args)
      (__steel-write-string (apply string-copy (cons string (cdr args))) (car args))))

(define (flush-output-port . port)
  (__steel-flush-output-port (if (null? port) (current-output-port) (car port))))

;; Pairs cannot be cyclic in Steel, so labels are never needed.
(define (write-shared obj . port) (apply write obj port))
(define (write-simple obj . port) (apply write obj port))

(define (current-second) (/ (current-inexact-milliseconds) 1000.0))
(define (jiffies-per-second) 1000)
(define (current-jiffy) (current-milliseconds))
(define (features) '(ratios exact-complex full-unicode block-schemer))
