import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val keystoreProperties = Properties().apply {
    val propFile = file("../keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.camacho.plasmahub"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.camacho.plasmahub"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (keystoreProperties.isNotEmpty()) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            if (keystoreProperties.isNotEmpty()) {
                signingConfig = signingConfigs.getByName("release")
            }
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

val tdlibNativeLibs = mapOf(
    "aarch64" to ("aarch64-linux-android" to "arm64-v8a"),
    "armv7" to ("armv7-linux-androideabi" to "armeabi-v7a"),
    "i686" to ("i686-linux-android" to "x86"),
    "x86_64" to ("x86_64-linux-android" to "x86_64"),
)

afterEvaluate {
    val targetsList = (findProperty("targetList") as? String)?.split(',') ?: listOf("aarch64", "armv7", "i686", "x86_64")

    for (profile in listOf("debug", "release")) {
        val profileCapitalized = profile.replaceFirstChar { it.uppercase() }
        val copyTask = tasks.register("copyTdlib${profileCapitalized}NativeLibs") {
            group = "rust"
            description = "Copy TDLib shared libraries into Android jniLibs for $profile builds"
            dependsOn("rustBuildUniversal$profileCapitalized")

            doLast {
                for (targetName in targetsList) {
                    val (triple, abi) = tdlibNativeLibs[targetName] ?: continue
                    val buildDir = file("../../../target/$triple/$profile/build")
                    val tdjson = fileTree(buildDir) {
                        include("**/out/tdlib/lib/libtdjson.so")
                    }.files.maxByOrNull { it.lastModified() }
                        ?: throw GradleException("libtdjson.so not found for $targetName in $buildDir")

                    copy {
                        from(tdjson)
                        into(file("src/main/jniLibs/$abi"))
                    }
                }
            }
        }

        tasks.matching { it.name == "mergeUniversal${profileCapitalized}JniLibFolders" }.configureEach {
            dependsOn(copyTask)
        }

        for (targetName in targetsList) {
            val abi = tdlibNativeLibs[targetName]?.second ?: continue
            val archName = when (abi) {
                "arm64-v8a" -> "Arm64"
                "armeabi-v7a" -> "Arm"
                "x86" -> "X86"
                "x86_64" -> "X86_64"
                else -> continue
            }
            tasks.matching { it.name == "merge$archName${profileCapitalized}JniLibFolders" }.configureEach {
                dependsOn(copyTask)
            }
        }
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
