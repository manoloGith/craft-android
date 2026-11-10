// craft-activity: the Kotlin half of craft-android (CraftActivity and its helpers), as an Android
// library module the apps include from this repository (see docs/integrate.md).
plugins {
    id("com.android.library")
}

android {
    namespace = "io.github.craftandroid"
    compileSdk = 36

    defaultConfig {
        minSdk = 29
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // GameActivity's Java side, the version android-activity 0.6 supports. Its prefab C++ glue
    // stays off: android-activity compiles its own.
    api("androidx.games:games-activity:4.4.0")
    api("androidx.appcompat:appcompat:1.7.1")
}
