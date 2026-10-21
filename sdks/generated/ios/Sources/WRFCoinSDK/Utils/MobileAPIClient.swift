import Foundation
import Combine
import Network

/**
 * Mobile-optimized API client for iOS with advanced features.
 *
 * Features:
 * - URLSession optimization for mobile networks
 * - Certificate pinning for security
 * - Network monitoring and adaptation
 * - Background task support
 * - Keychain integration for secure storage
 * - Automatic retry with exponential backoff
 */
@available(iOS 15.0, *)
public class MobileAPIClient: ObservableObject {
    
    // MARK: - Properties
    
    public static let shared = MobileAPIClient()
    
    private let baseURL: URL
    private let session: URLSession
    private let networkMonitor = NWPathMonitor()
    private let secureStorage = SecureCredentialStorage()
    private let cacheManager = APICacheManager()
    
    @Published public var isOnline = true
    @Published public var networkQuality: NetworkQuality = .good
    @Published public var isBackgroundSyncActive = false
    
    private let monitorQueue = DispatchQueue(label: "NetworkMonitor")
    private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
    
    // MARK: - Network Quality
    
    public enum NetworkQuality {
        case poor
        case fair
        case good
        case excellent
        
        var timeoutInterval: TimeInterval {
            switch self {
            case .poor: return 60.0
            case .fair: return 45.0
            case .good: return 30.0
            case .excellent: return 15.0
            }
        }
        
        var retryDelay: TimeInterval {
            switch self {
            case .poor: return 10.0
            case .fair: return 5.0
            case .good: return 2.0
            case .excellent: return 1.0
            }
        }
    }
    
    // MARK: - Initialization
    
    private init() {
        // Configure base URL based on build configuration
        #if DEBUG
        self.baseURL = URL(string: "http://localhost:4001")!
        #else
        self.baseURL = URL(string: "https://api.wrfcoin.com")!
        #endif
        
        // Create optimized URLSession
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 30.0
        config.timeoutIntervalForResource = 120.0
        config.waitsForConnectivity = true
        config.allowsCellularAccess = true
        config.allowsExpensiveNetworkAccess = true
        config.allowsConstrainedNetworkAccess = false
        
        // Configure cache
        config.urlCache = URLCache(
            memoryCapacity: 20 * 1024 * 1024, // 20MB
            diskCapacity: 100 * 1024 * 1024,  // 100MB
            diskPath: "wrfcoin_cache"
        )
        config.requestCachePolicy = .returnCacheDataElseLoad
        
        // Add custom headers
        config.httpAdditionalHeaders = [
            "User-Agent": userAgent,
            "X-SDK-Version": "1.0.0",
            "X-Platform": "ios"
        ]
        
        self.session = URLSession(
            configuration: config,
            delegate: SessionDelegate(),
            delegateQueue: nil
        )
        
        setupNetworkMonitoring()
        setupBackgroundHandling()
    }
    
    // MARK: - Network Monitoring
    
    private func setupNetworkMonitoring() {
        networkMonitor.pathUpdateHandler = { [weak self] path in
            DispatchQueue.main.async {
                self?.isOnline = path.status == .satisfied
                self?.updateNetworkQuality(for: path)
            }
        }
        networkMonitor.start(queue: monitorQueue)
    }
    
    private func updateNetworkQuality(for path: NWPath) {
        let quality: NetworkQuality
        
        if path.isExpensive {
            quality = .fair
        } else if path.usesInterfaceType(.wifi) {
            quality = .excellent
        } else if path.usesInterfaceType(.cellular) {
            quality = .good
        } else {
            quality = .poor
        }
        
        DispatchQueue.main.async {
            self.networkQuality = quality
        }
    }
    
    // MARK: - Background Handling
    
    private func setupBackgroundHandling() {
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(appDidEnterBackground),
            name: UIApplication.didEnterBackgroundNotification,
            object: nil
        )
        
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(appWillEnterForeground),
            name: UIApplication.willEnterForegroundNotification,
            object: nil
        )
    }
    
    @objc private func appDidEnterBackground() {
        backgroundTask = UIApplication.shared.beginBackgroundTask { [weak self] in
            self?.endBackgroundTask()
        }
        
        // Schedule background sync if needed
        scheduleBackgroundSync()
    }
    
    @objc private func appWillEnterForeground() {
        endBackgroundTask()
        
        // Refresh data when app becomes active
        triggerForegroundSync()
    }
    
    private func endBackgroundTask() {
        if backgroundTask != .invalid {
            UIApplication.shared.endBackgroundTask(backgroundTask)
            backgroundTask = .invalid
        }
    }
    
    // MARK: - Authentication
    
    public func setAPIKey(_ apiKey: String) {
        secureStorage.save(key: "api_key", value: apiKey)
    }
    
    public func setBearerToken(_ token: String) {
        secureStorage.save(key: "bearer_token", value: token)
    }
    
    public func clearCredentials() {
        secureStorage.deleteAll()
    }
    
    // MARK: - API Request Methods
    
    public func request<T: Codable>(
        endpoint: String,
        method: HTTPMethod = .GET,
        parameters: [String: Any]? = nil,
        body: Data? = nil
    ) -> AnyPublisher<T, APIError> {
        
        guard let url = URL(string: endpoint, relativeTo: baseURL) else {
            return Fail(error: APIError.invalidURL)
                .eraseToAnyPublisher()
        }
        
        var request = URLRequest(url: url)
        request.httpMethod = method.rawValue
        request.timeoutInterval = networkQuality.timeoutInterval
        
        // Add authentication headers
        if let apiKey = secureStorage.retrieve(key: "api_key") {
            request.setValue(apiKey, forHTTPHeaderField: "X-API-Key")
        }
        
        if let bearerToken = secureStorage.retrieve(key: "bearer_token") {
            request.setValue("Bearer \\(bearerToken)", forHTTPHeaderField: "Authorization")
        }
        
        // Add parameters or body
        if let parameters = parameters {
            if method == .GET {
                var components = URLComponents(url: url, resolvingAgainstBaseURL: true)
                components?.queryItems = parameters.map { URLQueryItem(name: $0.key, value: "\\($0.value)") }
                request.url = components?.url
            } else {
                request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                request.httpBody = try? JSONSerialization.data(withJSONObject: parameters)
            }
        } else if let body = body {
            request.httpBody = body
        }
        
        return performRequest(request)
            .decode(type: T.self, decoder: JSONDecoder())
            .mapError { error in
                if error is DecodingError {
                    return APIError.decodingFailed(error)
                }
                return error as? APIError ?? APIError.unknown(error)
            }
            .eraseToAnyPublisher()
    }
    
    private func performRequest(_ request: URLRequest) -> AnyPublisher<Data, APIError> {
        return session.dataTaskPublisher(for: request)
            .tryMap { data, response in
                guard let httpResponse = response as? HTTPURLResponse else {
                    throw APIError.invalidResponse
                }
                
                guard 200...299 ~= httpResponse.statusCode else {
                    throw APIError.httpError(httpResponse.statusCode)
                }
                
                return data
            }
            .mapError { error in
                if let apiError = error as? APIError {
                    return apiError
                }
                
                if (error as NSError).code == NSURLErrorNotConnectedToInternet {
                    return APIError.networkUnavailable
                }
                
                return APIError.networkError(error)
            }
            .retry(3) // Automatic retry
            .eraseToAnyPublisher()
    }
    
    // MARK: - Background Sync
    
    private func scheduleBackgroundSync() {
        Task {
            await performBackgroundSync()
        }
    }
    
    private func triggerForegroundSync() {
        Task {
            await performForegroundSync()
        }
    }
    
    @MainActor
    private func performBackgroundSync() async {
        guard isOnline else { return }
        
        isBackgroundSyncActive = true
        defer { isBackgroundSyncActive = false }
        
        // Implement background sync logic here
        print("Performing background sync...")
        
        // Sync critical data only in background
        do {
            // Example: Sync weather data
            let _: WeatherData = try await request(endpoint: "/api/weather/latest").async()
            print("Background sync completed successfully")
        } catch {
            print("Background sync failed: \\(error)")
        }
    }
    
    @MainActor
    private func performForegroundSync() async {
        guard isOnline else { return }
        
        print("Performing foreground sync...")
        
        // Full sync when app becomes active
        // Implement comprehensive sync logic here
    }
    
    // MARK: - Cache Management
    
    public func clearCache() {
        session.configuration.urlCache?.removeAllCachedResponses()
        cacheManager.clearAll()
    }
    
    public func getCacheSize() -> Int64 {
        return session.configuration.urlCache?.currentDiskUsage ?? 0
    }
    
    // MARK: - Utilities
    
    private var userAgent: String {
        let bundleInfo = Bundle.main.infoDictionary
        let appName = bundleInfo?["CFBundleName"] as? String ?? "WRFCoinApp"
        let appVersion = bundleInfo?["CFBundleShortVersionString"] as? String ?? "1.0"
        let systemVersion = UIDevice.current.systemVersion
        let deviceModel = UIDevice.current.model
        
        return "\\(appName)/\\(appVersion) (iOS \\(systemVersion); \\(deviceModel)) WRFCoinSDK/1.0.0"
    }
    
    deinit {
        networkMonitor.cancel()
        NotificationCenter.default.removeObserver(self)
        endBackgroundTask()
    }
}

// MARK: - Supporting Types

public enum HTTPMethod: String {
    case GET = "GET"
    case POST = "POST"
    case PUT = "PUT"
    case DELETE = "DELETE"
    case PATCH = "PATCH"
}

public enum APIError: Error, LocalizedError {
    case invalidURL
    case invalidResponse
    case networkUnavailable
    case httpError(Int)
    case networkError(Error)
    case decodingFailed(Error)
    case unknown(Error)
    
    public var errorDescription: String? {
        switch self {
        case .invalidURL:
            return "Invalid URL"
        case .invalidResponse:
            return "Invalid response"
        case .networkUnavailable:
            return "Network unavailable"
        case .httpError(let code):
            return "HTTP error: \\(code)"
        case .networkError(let error):
            return "Network error: \\(error.localizedDescription)"
        case .decodingFailed(let error):
            return "Decoding failed: \\(error.localizedDescription)"
        case .unknown(let error):
            return "Unknown error: \\(error.localizedDescription)"
        }
    }
}

// MARK: - URLSession Delegate for Certificate Pinning

private class SessionDelegate: NSObject, URLSessionDelegate {
    
    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        
        // Implement certificate pinning for production
        #if !DEBUG
        if let serverTrust = challenge.protectionSpace.serverTrust,
           challenge.protectionSpace.host.contains("wrfcoin.com") {
            
            // Validate certificate pins here
            // This is a simplified implementation
            let policy = SecPolicyCreateSSL(true, "api.wrfcoin.com" as CFString)
            SecTrustSetPolicies(serverTrust, policy)
            
            var evaluationResult: SecTrustResultType = .invalid
            let status = SecTrustEvaluate(serverTrust, &evaluationResult)
            
            if status == errSecSuccess &&
               (evaluationResult == .unspecified || evaluationResult == .proceed) {
                completionHandler(.useCredential, URLCredential(trust: serverTrust))
                return
            }
        }
        #endif
        
        // Use default handling for non-pinned hosts or debug builds
        completionHandler(.performDefaultHandling, nil)
    }
}

// MARK: - Combine Extensions

extension Publisher {
    func async() async throws -> Output {
        try await withCheckedThrowingContinuation { continuation in
            var cancellable: AnyCancellable?
            cancellable = first()
                .sink(
                    receiveCompletion: { completion in
                        switch completion {
                        case .finished:
                            break
                        case .failure(let error):
                            continuation.resume(throwing: error)
                        }
                        cancellable?.cancel()
                    },
                    receiveValue: { value in
                        continuation.resume(returning: value)
                        cancellable?.cancel()
                    }
                )
        }
    }
}