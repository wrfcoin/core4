import SwiftUI
import WRFCoinSDK
import MapKit

/**
 * Weather data management and visualization view.
 *
 * Features demonstrated:
 * - Weather station discovery and management
 * - Real-time weather data visualization
 * - Location-based weather services
 * - Data submission and validation
 * - Earnings tracking from weather contributions
 */
struct WeatherView: View {
    @State private var weatherData: [WeatherStation] = []
    @State private var selectedStation: WeatherStation?
    @State private var showingAddStation = false
    @State private var isLoading = false
    @State private var region = MKCoordinateRegion(
        center: CLLocationCoordinate2D(latitude: 37.7749, longitude: -122.4194),
        span: MKCoordinateSpan(latitudeDelta: 0.5, longitudeDelta: 0.5)
    )
    
    var body: some View {
        NavigationView {
            VStack(spacing: 0) {
                // Weather Map
                WeatherMapView(
                    region: $region,
                    weatherStations: weatherData,
                    selectedStation: $selectedStation
                )
                .frame(height: 300)
                
                // Weather Station List
                List {
                    Section("Your Weather Stations") {
                        ForEach(weatherData) { station in
                            WeatherStationRow(station: station)
                                .onTapGesture {
                                    selectedStation = station
                                }
                        }
                    }
                    
                    Section("Recent Submissions") {
                        ForEach(recentSubmissions) { submission in
                            SubmissionRow(submission: submission)
                        }
                    }
                }
            }
            .navigationTitle("Weather Stations")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button(action: { showingAddStation = true }) {
                        Image(systemName: "plus")
                    }
                }
            }
            .sheet(isPresented: $showingAddStation) {
                AddWeatherStationView()
            }
            .task {
                await loadWeatherData()
            }
        }
    }
    
    private func loadWeatherData() async {
        isLoading = true
        
        // Simulate loading weather station data
        try? await Task.sleep(nanoseconds: 1_000_000_000)
        
        await MainActor.run {
            weatherData = sampleWeatherStations
            isLoading = false
        }
    }
    
    // Sample data
    private var sampleWeatherStations: [WeatherStation] {
        [
            WeatherStation(
                id: "1",
                name: "Home Station",
                location: CLLocationCoordinate2D(latitude: 37.7749, longitude: -122.4194),
                lastUpdate: Date(),
                temperature: 22.5,
                humidity: 65,
                status: .active
            ),
            WeatherStation(
                id: "2",
                name: "Garden Station",
                location: CLLocationCoordinate2D(latitude: 37.7849, longitude: -122.4094),
                lastUpdate: Date().addingTimeInterval(-3600),
                temperature: 24.1,
                humidity: 58,
                status: .active
            )
        ]
    }
    
    private var recentSubmissions: [WeatherSubmission] {
        [
            WeatherSubmission(
                id: "1",
                stationId: "1",
                timestamp: Date(),
                temperature: 22.5,
                humidity: 65,
                reward: 0.05
            ),
            WeatherSubmission(
                id: "2",
                stationId: "2",
                timestamp: Date().addingTimeInterval(-1800),
                temperature: 24.1,
                humidity: 58,
                reward: 0.04
            )
        ]
    }
}

/**
 * Weather map view showing station locations.
 */
struct WeatherMapView: UIViewRepresentable {
    @Binding var region: MKCoordinateRegion
    let weatherStations: [WeatherStation]
    @Binding var selectedStation: WeatherStation?
    
    func makeUIView(context: Context) -> MKMapView {
        let mapView = MKMapView()
        mapView.delegate = context.coordinator
        mapView.region = region
        mapView.showsUserLocation = true
        return mapView
    }
    
    func updateUIView(_ mapView: MKMapView, context: Context) {
        // Update annotations
        mapView.removeAnnotations(mapView.annotations.filter { !($0 is MKUserLocation) })
        
        let annotations = weatherStations.map { station in
            WeatherStationAnnotation(station: station)
        }
        
        mapView.addAnnotations(annotations)
    }
    
    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }
    
    class Coordinator: NSObject, MKMapViewDelegate {
        var parent: WeatherMapView
        
        init(_ parent: WeatherMapView) {
            self.parent = parent
        }
        
        func mapView(_ mapView: MKMapView, viewFor annotation: MKAnnotation) -> MKAnnotationView? {
            guard let stationAnnotation = annotation as? WeatherStationAnnotation else {
                return nil
            }
            
            let identifier = "WeatherStation"
            var annotationView = mapView.dequeueReusableAnnotationView(withIdentifier: identifier)
            
            if annotationView == nil {
                annotationView = MKPinAnnotationView(annotation: annotation, reuseIdentifier: identifier)
                annotationView?.canShowCallout = true
            }
            
            annotationView?.annotation = annotation
            
            // Set pin color based on station status
            if let pinView = annotationView as? MKPinAnnotationView {
                pinView.pinTintColor = stationAnnotation.station.status == .active ? .green : .red
            }
            
            return annotationView
        }
        
        func mapView(_ mapView: MKMapView, didSelect view: MKAnnotationView) {
            if let stationAnnotation = view.annotation as? WeatherStationAnnotation {
                parent.selectedStation = stationAnnotation.station
            }
        }
    }
}

/**
 * Weather station annotation for map.
 */
class WeatherStationAnnotation: NSObject, MKAnnotation {
    let station: WeatherStation
    
    var coordinate: CLLocationCoordinate2D {
        return station.location
    }
    
    var title: String? {
        return station.name
    }
    
    var subtitle: String? {
        return "\\(station.temperature)°C, \\(station.humidity)% humidity"
    }
    
    init(station: WeatherStation) {
        self.station = station
    }
}

/**
 * Weather station row in list.
 */
struct WeatherStationRow: View {
    let station: WeatherStation
    
    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(station.name)
                    .font(.headline)
                
                Text("\\(station.temperature, specifier: "%.1f")°C, \\(station.humidity)% humidity")
                    .font(.caption)
                    .foregroundColor(.secondary)
                
                Text("Updated \\(station.lastUpdate, style: .relative) ago")
                    .font(.caption2)
                    .foregroundColor(.secondary)
            }
            
            Spacer()
            
            VStack(alignment: .trailing) {
                Circle()
                    .fill(station.status == .active ? Color.green : Color.red)
                    .frame(width: 12, height: 12)
                
                Text(station.status.rawValue)
                    .font(.caption2)
                    .foregroundColor(.secondary)
            }
        }
        .padding(.vertical, 4)
    }
}

/**
 * Weather submission row.
 */
struct SubmissionRow: View {
    let submission: WeatherSubmission
    
    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text("Station \\(submission.stationId)")
                    .font(.headline)
                
                Text("\\(submission.temperature, specifier: "%.1f")°C, \\(submission.humidity)% humidity")
                    .font(.caption)
                    .foregroundColor(.secondary)
                
                Text(submission.timestamp, style: .relative)
                    .font(.caption2)
                    .foregroundColor(.secondary)
            }
            
            Spacer()
            
            VStack(alignment: .trailing) {
                Text("+\\(submission.reward, specifier: "%.3f") WRF")
                    .font(.caption)
                    .foregroundColor(.green)
                
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.green)
                    .font(.caption)
            }
        }
        .padding(.vertical, 4)
    }
}

/**
 * Add weather station view.
 */
struct AddWeatherStationView: View {
    @Environment(\\.dismiss) var dismiss
    @State private var stationName = ""
    @State private var selectedLocation: CLLocationCoordinate2D?
    @State private var showingLocationPicker = false
    
    var body: some View {
        NavigationView {
            Form {
                Section("Station Details") {
                    TextField("Station Name", text: $stationName)
                    
                    Button("Select Location") {
                        showingLocationPicker = true
                    }
                    
                    if let location = selectedLocation {
                        Text("Location: \\(location.latitude, specifier: "%.4f"), \\(location.longitude, specifier: "%.4f")")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
                
                Section("Sensor Configuration") {
                    Toggle("Temperature Sensor", isOn: .constant(true))
                    Toggle("Humidity Sensor", isOn: .constant(true))
                    Toggle("Pressure Sensor", isOn: .constant(false))
                    Toggle("Wind Speed Sensor", isOn: .constant(false))
                }
            }
            .navigationTitle("Add Weather Station")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Add") {
                        addWeatherStation()
                    }
                    .disabled(stationName.isEmpty || selectedLocation == nil)
                }
            }
        }
    }
    
    private func addWeatherStation() {
        // Add weather station using SDK
        print("Adding weather station: \\(stationName)")
        dismiss()
    }
}

// MARK: - Data Models

struct WeatherStation: Identifiable {
    let id: String
    let name: String
    let location: CLLocationCoordinate2D
    let lastUpdate: Date
    let temperature: Double
    let humidity: Int
    let status: StationStatus
    
    enum StationStatus: String, CaseIterable {
        case active = "Active"
        case inactive = "Inactive"
        case maintenance = "Maintenance"
    }
}

struct WeatherSubmission: Identifiable {
    let id: String
    let stationId: String
    let timestamp: Date
    let temperature: Double
    let humidity: Int
    let reward: Double
}

#Preview {
    WeatherView()
}