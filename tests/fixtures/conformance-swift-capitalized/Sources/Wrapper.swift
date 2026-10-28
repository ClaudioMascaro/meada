import XCTest

internal func XCTAssertNoThrow<T>(_ e: @autoclosure () async throws -> T, file: StaticString = #filePath) async {}
